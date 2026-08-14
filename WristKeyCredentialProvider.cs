using System;
using System.IO.Pipes;
using System.Security.Principal;
using System.Text;
using System.Runtime.InteropServices;
using CredProvInterop;

namespace WristKeyCredentialProvider
{
    [ComVisible(true)]
    [Guid("A1B2C3D4-E5F6-7890-ABCD-EF1234567895")]
    [ClassInterface(ClassInterfaceType.None)]
    public class WristKeyCredentialProvider : ICredentialProvider
    {
        private ICredentialProviderEvents _events;
        private string _userName;
        private string _password;

        public int SetUsageScenario(CREDENTIAL_PROVIDER_USAGE_SCENARIO cpus, uint dwFlags)
        {
            // Only show on unlock/workstation unlock
            if (cpus == CREDENTIAL_PROVIDER_USAGE_SCENARIO.CPUS_LOGON ||
                cpus == CREDENTIAL_PROVIDER_USAGE_SCENARIO.CPUS_UNLOCK_WORKSTATION)
            {
                return HRESULT.S_OK;
            }
            return HRESULT.E_NOTIMPL;
        }

        public int SetUserSid(string pszUserSid) => HRESULT.S_OK;

        public int Advise(ICredentialProviderEvents pcpe, ulong upAdviseContext)
        {
            _events = pcpe;
            return HRESULT.S_OK;
        }

        public int UnAdvise() { _events = null; return HRESULT.S_OK; }

        public int GetFieldDescriptorCount(out uint pdwCount)
        {
            pdwCount = 2; // Tile + status text
            return HRESULT.S_OK;
        }

        public int GetFieldDescriptorAt(uint dwIndex, out IntPtr ppcpfd)
        {
            ppcpfd = IntPtr.Zero;
            return HRESULT.S_OK;
        }

        public int GetCredentialCount(out uint pdwCount, out uint pdwDefault, out int pbAutoLogonWithDefault)
        {
            pdwCount = 1;
            pdwDefault = unchecked((uint)-1);
            pbAutoLogonWithDefault = 0;
            return HRESULT.S_OK;
        }

        public int GetCredentialAt(uint dwIndex, out ICredentialProviderCredential ppcpc)
        {
            ppcpc = new WristKeyCredential();
            return HRESULT.S_OK;
        }
    }

    [ComVisible(true)]
    [ClassInterface(ClassInterfaceType.None)]
    public class WristKeyCredential : ICredentialProviderCredential
    {
        public int Advise(ICredentialProviderCredentialEvents pcpce) => HRESULT.S_OK;
        public int UnAdvise() => HRESULT.S_OK;
        public int SetSelected(out int pbAutoLogon) { pbAutoLogon = 0; return HRESULT.S_OK; }
        public int SetDeselected() => HRESULT.S_OK;

        public int GetFieldState(uint dwFieldId, out CREDENTIAL_PROVIDER_FIELD_STATE pcpfs, out CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE pcpfis)
        {
            pcpfs = CREDENTIAL_PROVIDER_FIELD_STATE.CPFS_DISPLAY_IN_SELECTED_TILE;
            pcpfis = CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE.CPFIS_NONE;
            return HRESULT.S_OK;
        }

        public int GetStringValue(uint dwFieldId, out string ppsz)
        {
            ppsz = dwFieldId == 0 ? "🔓 WristKey Unlock" : "Bring your watch close…";
            return HRESULT.S_OK;
        }

        public int GetBitmapValue(uint dwFieldId, out IntPtr phbmp)
        {
            phbmp = IntPtr.Zero;
            return HRESULT.E_NOTIMPL;
        }

        public int GetCheckboxValue(uint dwFieldId, out int pbChecked, out string ppszLabel)
        {
            pbChecked = 0;
            ppszLabel = null;
            return HRESULT.E_NOTIMPL;
        }

        public int GetSubmitButtonValue(uint dwFieldId, out uint pdwAdjacentTo)
        {
            pdwAdjacentTo = 0;
            return HRESULT.E_NOTIMPL;
        }

        public int GetComboBoxValueCount(uint dwFieldId, out uint pcItems, out uint pdwSelectedItem)
        {
            pcItems = 0;
            pdwSelectedItem = 0;
            return HRESULT.E_NOTIMPL;
        }

        public int GetComboBoxValueAt(uint dwFieldId, uint dwItem, out string ppszItem)
        {
            ppszItem = null;
            return HRESULT.E_NOTIMPL;
        }

        public int Advise(ICredentialProviderCredentialEvents pcpce, out IntPtr phbmp)
        {
            phbmp = IntPtr.Zero;
            return HRESULT.E_NOTIMPL;
        }

        public int GetSerialization(out CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE pcpgsr,
            out CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION pcpcs, out string ppszOptionalStatusText,
            out int pcpsiOptionalStatusIcon)
        {
            pcpgsr = CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE.CPGSR_RETURN_CREDENTIAL_FINISHED;
            pcpcs = new CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION();
            ppszOptionalStatusText = null;
            pcpsiOptionalStatusIcon = 0;

            try
            {
                string password = ReadPasswordFromPipe();
                if (string.IsNullOrEmpty(password))
                {
                    pcpgsr = CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE.CPGSR_NO_CREDENTIAL_NOT_FINISHED;
                    return HRESULT.E_FAIL;
                }

                // Build KERB_INTERACTIVE_UNLOCK_LOGON
                byte[] serialized = SerializeLogon("", password);
                pcpcs.clsidCredentialProvider = new Guid("A1B2C3D4-E5F6-7890-ABCD-EF1234567895");
                pcpcs.rgbSerialization = serialized;
                pcpcs.ulAuthenticationPackage = GetAuthenticationPackageId();
                pcpcs.cbSerialization = (uint)serialized.Length;
                pcpcs.CredentialBlobSize = (uint)Encoding.Unicode.GetByteCount(password);
                pcpcs.CredentialBlob = Marshal.StringToCoTaskMemUni(password);

                return HRESULT.S_OK;
            }
            catch (Exception ex)
            {
                ppszOptionalStatusText = $"WristKey error: {ex.Message}";
                pcpsiOptionalStatusIcon = (int)CREDENTIAL_PROVIDER_STATUS_ICON.CPSI_ERROR;
                return HRESULT.E_FAIL;
            }
        }

        public int ReportResult(int ntsStatus, int ntsSubstatus, out string ppszOptionalStatusText, out int pcpsiOptionalStatusIcon)
        {
            ppszOptionalStatusText = null;
            pcpsiOptionalStatusIcon = 0;
            return HRESULT.S_OK;
        }

        private string ReadPasswordFromPipe()
        {
            try
            {
                using (var client = new NamedPipeClientStream(".", "WristKeyUnlock", PipeDirection.In, PipeOptions.None, TokenImpersonationLevel.Impersonation))
                {
                    client.Connect(2000);
                    using (var reader = new StreamReader(client, Encoding.UTF8))
                    {
                        return reader.ReadLine();
                    }
                }
            }
            catch { return null; }
        }

        private byte[] SerializeLogon(string username, string password)
        {
            // Simplified — real implementation needs KERB_INTERACTIVE_UNLOCK_LOGON structure
            return Encoding.Unicode.GetBytes($"{username}\0{password}\0");
        }

        private uint GetAuthenticationPackageId()
        {
            // Should query LSA for Kerberos package ID
            return 2; // NTLM/Kerberos placeholder
        }
    }

    // COM Interop stubs (simplified — use Lithnet.CredentialProvider or full interop in production)
    public static class HRESULT
    {
        public const int S_OK = 0;
        public const int E_NOTIMPL = unchecked((int)0x80004001);
        public const int E_FAIL = unchecked((int)0x80004005);
    }

    public enum CREDENTIAL_PROVIDER_USAGE_SCENARIO { CPUS_LOGON = 1, CPUS_UNLOCK_WORKSTATION = 2 }
    public enum CREDENTIAL_PROVIDER_FIELD_STATE { CPFS_DISPLAY_IN_SELECTED_TILE = 1 }
    public enum CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE { CPFIS_NONE = 0 }
    public enum CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE { CPGSR_NO_CREDENTIAL_NOT_FINISHED = 0, CPGSR_RETURN_CREDENTIAL_FINISHED = 1 }
    public enum CREDENTIAL_PROVIDER_STATUS_ICON { CPSI_NONE = 0, CPSI_ERROR = 3 }

    [ComImport, Guid("6C793AA0-1B8A-4A7B-8C3B-9B5F3E7A7E1E")]
    public interface ICredentialProvider { /* ... */ }
    [ComImport, Guid("C27C8D5E-776A-4A85-8Bfd-3F0F4C3B2A1E")]
    public interface ICredentialProviderCredential { /* ... */ }
    [ComImport, Guid("F1F3D5E7-9B8A-4C2D-E6F0-1A3B5C7D9E0F")]
    public interface ICredentialProviderEvents { /* ... */ }
    [ComImport, Guid("A1B2C3D4-E5F6-7890-ABCD-EF1234567896")]
    public interface ICredentialProviderCredentialEvents { /* ... */ }

    [StructLayout(LayoutKind.Sequential)]
    public struct CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION
    {
        public uint ulAuthenticationPackage;
        public Guid clsidCredentialProvider;
        public uint cbSerialization;
        public IntPtr rgbSerialization;
        public IntPtr CredentialBlob;
        public uint CredentialBlobSize;
    }
}
