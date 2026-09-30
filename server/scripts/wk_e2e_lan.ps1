param(
    [Parameter(Mandatory=$true)][string]$ServerExe,
    [string]$Listen = "127.0.0.1:8787",
    [string]$Token = "e2e-lan-token-12345"
)

$ErrorActionPreference = "Stop"
$Pass = 0; $Fail = 0

function Ok($name, $cond, $extra = "") {
    if ($cond) { $script:Pass++; Write-Host "[PASS] $name $extra" }
    else       { $script:Fail++; Write-Host "[FAIL] $name $extra" }
}

# --- ECDSA P-256 helpers (mirror the watch KeyStoreManager: SEC1 04||X||Y pubkey, raw R||S signature, SHA-256) ---
$curve = [System.Security.Cryptography.ECCurve]::CreateFromFriendlyName("nistP256")
$ecdsa = [System.Security.Cryptography.ECDsa]::Create()
$ecdsa.GenerateKey($curve)

function BuildSec1([System.Security.Cryptography.ECDsa]$e) {
    $p = $e.ExportParameters($false)
    function Pad32([byte[]]$b) {
        $out = New-Object byte[] 32
        [System.Array]::Copy($b, ([Math]::Max(0, $b.Length - 32)), $out, ([Math]::Max(0, 32 - $b.Length)), [Math]::Min(32, $b.Length))
        return $out
    }
    $x = Pad32 $p.Q.X; $y = Pad32 $p.Q.Y
    $pk = New-Object byte[] 65; $pk[0] = 0x04
    [System.Array]::Copy($x, 0, $pk, 1, 32); [System.Array]::Copy($y, 0, $pk, 33, 32)
    return $pk
}

# DER -> raw R||S (64 bytes), same as the watch KeyStoreManager.derToRawSignature
function DerToRaw([byte[]]$der) {
    $i = 2
    if ($der[1] -band 0x80) { $i = 2 + ($der[1] -band 0x7f) }
    if ($der[$i] -ne 0x02) { throw "bad DER R tag" }
    $rLen = $der[$i+1]; $r = $der[($i+2)..($i+1+$rLen)]
    if ($r.Length -gt 32 -and $r[0] -eq 0) { $r = $r[1..($r.Length-1)] }
    $i = $i + 2 + $rLen
    if ($der[$i] -ne 0x02) { throw "bad DER S tag" }
    $sLen = $der[$i+1]; $s = $der[($i+2)..($i+1+$sLen)]
    if ($s.Length -gt 32 -and $s[0] -eq 0) { $s = $s[1..($s.Length-1)] }
    function Pad32b([byte[]]$b) { $out = New-Object byte[] 32; [System.Array]::Copy($b, ([Math]::Max(0,$b.Length-32)), $out, ([Math]::Max(0,32-$b.Length)), [Math]::Min(32,$b.Length)); $out }
    return (Pad32b $r) + (Pad32b $s)
}

Add-Type -AssemblyName System.Net.Http

# --- HTTP helpers (mirror the watch LanClient: /api/v1/*, Bearer, JSON) ---
$client = New-Object System.Net.Http.HttpClient
$client.Timeout = [TimeSpan]::FromSeconds(10)

function Invoke-White($method, $path, $bodyObj = $null, [bool]$withToken = $true) {
    $url = "http://$Listen$path"
    $req = New-Object System.Net.Http.HttpRequestMessage(([System.Net.Http.HttpMethod]::$method), $url)
    if ($withToken) { $req.Headers.Authorization = New-Object System.Net.Http.Headers.AuthenticationHeaderValue("Bearer", $Token) }
    if ($null -ne $bodyObj) {
        $json = $bodyObj | ConvertTo-Json -Compress
        $req.Content = New-Object System.Net.Http.StringContent($json, [System.Text.Encoding]::UTF8, "application/json")
    }
    $resp = $client.SendAsync($req).Result
    $body = $resp.Content.ReadAsStringAsync().Result
    return [pscustomobject]@{ status = [int]$resp.StatusCode; body = $body }
}

# --- Start wristkey-msa (lan mode requires a token) ---
$proc = Start-Process -FilePath $ServerExe -ArgumentList @("--mode","lan","--listen",$Listen,"--token",$Token) -PassThru -WindowStyle Hidden
try {
    $up = $false
    for ($i = 0; $i -lt 50; $i++) {
        Start-Sleep -Milliseconds 200
        try { $r = Invoke-White "Get" "/api/v1/health" $null $false; if ($r.status -eq 200) { $up = $true; break } } catch {}
    }
    Ok "server up" $up "(health on $Listen)"

    # 1. health is PUBLIC (no token) -> 200
    $h = Invoke-White "Get" "/api/v1/health" $null $false
    Ok "health public" ($h.status -eq 200 -and $h.body -eq "ok") "-> $($h.status) $($h.body)"

    # 2. challenge (with token) -> nonce_b64
    $ch = Invoke-White "Post" "/api/v1/challenge" @{}
    $nonceJson = $null
    try { $nonceJson = $ch.body | ConvertFrom-Json } catch {}
    $nonceB64 = $nonceJson.nonce_b64
    Ok "challenge with token" ($ch.status -eq 200 -and $null -ne $nonceB64) "-> $($ch | ConvertTo-Json -Compress)"

    # 3. challenge WITHOUT token -> 401
    $chNo = Invoke-White "Post" "/api/v1/challenge" @{} $false
    Ok "challenge without token 401" ($chNo.status -eq 401) "-> $($chNo.status)"

    # 4. register: message = "wristkey-msa/register/v1" || nonce (LanClient REGISTER_DOMAIN)
    $nonce = [Convert]::FromBase64String($nonceB64)
    $domain = [System.Text.Encoding]::UTF8.GetBytes("wristkey-msa/register/v1")
    $message = $domain + $nonce
    $raw = $ecdsa.SignData($message, [System.Security.Cryptography.HashAlgorithmName]::SHA256)
    # Some .NET runtimes return DER (0x30...) and the watch converts DER->raw; CNG here returns raw R||S already.
    $sig = if ($raw.Length -gt 64 -and $raw[0] -eq 0x30) { DerToRaw $raw } else { $raw }
    $pubB64 = [Convert]::ToBase64String((BuildSec1 $ecdsa))
    $sigB64 = [Convert]::ToBase64String($sig)
    $regBody = @{
        pc_name             = "DESK-E2E-NOBT"
        watch_pubkey_b64    = $pubB64
        msa_account         = "e2e@wristkey.local"
        nonce_b64           = $nonceB64
        signature_b64       = $sigB64
    }
    $reg = Invoke-White "Post" "/api/v1/register" $regBody
    $regJson = $reg.body | ConvertFrom-Json
    Ok "register with valid sig" ($reg.status -eq 200 -and $null -ne $regJson.wristkey_id) "-> $($reg.status) wristkey_id=$($regJson.wristkey_id)"

    # 5. register WITHOUT token -> 401
    $regNo = Invoke-White "Post" "/api/v1/register" $regBody $false
    Ok "register without token 401" ($regNo.status -eq 401) "-> $($regNo.status)"

    # 6. register with WRONG sig (same nonce) -> 401
    $bad = @{ pc_name = "DESK-E2E-NOBT"; watch_pubkey_b64 = $pubB64; msa_account = "e2e@wristkey.local"; nonce_b64 = $nonceB64; signature_b64 = [Convert]::ToBase64String((New-Object byte[] 64)) }
    $regBad = Invoke-White "Post" "/api/v1/register" $bad
    Ok "register bad sig 401" ($regBad.status -eq 401) "-> $($regBad.status)"

    # 7. status/{id} -> 200 with binding
    $st = Invoke-White "Get" "/api/v1/status/$($regJson.wristkey_id)"
    $stJson = $st.body | ConvertFrom-Json
    Ok "status binding" ($st.status -eq 200 -and $stJson.msa_account -eq "e2e@wristkey.local") "-> $($st.status) pc=$($stJson.pc_name)"

    # 8. register REPLAY (same nonce already consumed) -> 401
    $rep = Invoke-White "Post" "/api/v1/register" $regBody
    Ok "register replay 401" ($rep.status -eq 401) "-> $($rep.status)"

    # 9. unknown id -> 404
    $st404 = Invoke-White "Get" "/api/v1/status/wk-11111111-1111-4111-8111-111111111111"
    Ok "unknown wristkey_id 404" ($st404.status -eq 404) "-> $($st404.status)"
}
finally {
    $client.Dispose()
    if ($proc -and -not $proc.HasExited) { Stop-Process -Id $proc.Id -Force }
}

Write-Host ""
Write-Host "RESULT: PASS=$Pass FAIL=$Fail"
if ($Fail -gt 0) { exit 1 } else { exit 0 }