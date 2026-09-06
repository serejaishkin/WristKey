#pragma once
#include <windows.h>
#include <initguid.h>
#include <credentialprovider.h>
#include <string>
#include <vector>

// {7E1B7B8A-4C8B-4C2F-9D8A-8D3A7F2E51A1}
DEFINE_GUID(CLSID_WristKeyCredentialProvider,
    0x7e1b7b8a, 0x4c8b, 0x4c2f, 0x9d, 0x8a, 0x8d, 0x3a, 0x7f, 0x2e, 0x51, 0xa1);

class WristKeyProvider;

class WristKeyProviderCredential final : public ICredentialProviderCredential2 {
public:
    WristKeyProviderCredential(WristKeyProvider* provider, std::wstring username, std::wstring sid);
    ~WristKeyProviderCredential() = default;

    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID riid, void** ppv) override;
    ULONG STDMETHODCALLTYPE AddRef() override;
    ULONG STDMETHODCALLTYPE Release() override;
    HRESULT STDMETHODCALLTYPE Advise(ICredentialProviderCredentialEvents* pcpce) override;
    HRESULT STDMETHODCALLTYPE UnAdvise() override;
    HRESULT STDMETHODCALLTYPE SetSelected(BOOL* pbAutoLogon) override;
    HRESULT STDMETHODCALLTYPE SetDeselected() override;
    HRESULT STDMETHODCALLTYPE GetFieldState(DWORD dwFieldID, CREDENTIAL_PROVIDER_FIELD_STATE* pcpfs,
                                             CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE* pcpfis) override;
    HRESULT STDMETHODCALLTYPE GetStringValue(DWORD dwFieldID, PWSTR* ppwsz) override;
    HRESULT STDMETHODCALLTYPE GetBitmapValue(DWORD dwFieldID, HBITMAP* phbmp) override;
    HRESULT STDMETHODCALLTYPE GetCheckboxValue(DWORD dwFieldID, BOOL* pbChecked, LPWSTR* ppszLabel) override;
    HRESULT STDMETHODCALLTYPE GetSubmitButtonValue(DWORD dwFieldID, DWORD* pdwAdjacentTo) override;
    HRESULT STDMETHODCALLTYPE GetComboBoxValueCount(DWORD dwFieldID, DWORD* pcItems, DWORD* pdwSelectedItem) override;
    HRESULT STDMETHODCALLTYPE GetComboBoxValueAt(DWORD dwFieldID, DWORD dwItem, PWSTR* ppwszItem) override;
    HRESULT STDMETHODCALLTYPE SetStringValue(DWORD dwFieldID, PCWSTR pwz) override;
    HRESULT STDMETHODCALLTYPE SetCheckboxValue(DWORD dwFieldID, BOOL bChecked) override;
    HRESULT STDMETHODCALLTYPE SetComboBoxSelectedValue(DWORD dwFieldID, DWORD dwSelectedItem) override;
    HRESULT STDMETHODCALLTYPE CommandLinkClicked(DWORD dwFieldID) override;
    HRESULT STDMETHODCALLTYPE GetSerialization(CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE* pcpgsr,
                                               CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION* pcpcs,
                                               PWSTR* ppwszOptionalStatusText,
                                               CREDENTIAL_PROVIDER_STATUS_ICON* pcpsiOptionalStatusIcon) override;
    HRESULT STDMETHODCALLTYPE ReportResult(NTSTATUS ntsStatus, NTSTATUS ntsSubstatus,
                                           PWSTR* ppwszOptionalStatusText,
                                           CREDENTIAL_PROVIDER_STATUS_ICON* pcpsiOptionalStatusIcon) override;

    HRESULT STDMETHODCALLTYPE GetUserSid(PWSTR* ppszSid) override;

    const std::wstring& Username() const { return _username; }

private:
    LONG _ref = 1;
    WristKeyProvider* _provider = nullptr;
    ICredentialProviderCredentialEvents* _events = nullptr;
    std::wstring _username;
    std::wstring _sid;
};

class WristKeyProvider final : public ICredentialProvider {
public:
    WristKeyProvider();
    ~WristKeyProvider();

    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID riid, void** ppv) override;
    ULONG STDMETHODCALLTYPE AddRef() override;
    ULONG STDMETHODCALLTYPE Release() override;
    HRESULT STDMETHODCALLTYPE SetUsageScenario(CREDENTIAL_PROVIDER_USAGE_SCENARIO cpus, DWORD dwFlags) override;
    HRESULT STDMETHODCALLTYPE SetSerialization(const CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION* pcpcs) override;
    HRESULT STDMETHODCALLTYPE Advise(ICredentialProviderEvents* pcpe, UINT_PTR upAdviseContext) override;
    HRESULT STDMETHODCALLTYPE UnAdvise() override;
    HRESULT STDMETHODCALLTYPE GetFieldDescriptorCount(DWORD* pdwCount) override;
    HRESULT STDMETHODCALLTYPE GetFieldDescriptorAt(DWORD dwIndex, CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR** ppcpfd) override;
    HRESULT STDMETHODCALLTYPE GetCredentialCount(DWORD* pdwCount, DWORD* pdwDefault, BOOL* pbAutoLogonWithDefault) override;
    HRESULT STDMETHODCALLTYPE GetCredentialAt(DWORD dwIndex, ICredentialProviderCredential** ppcpc) override;

    void SetEvents(ICredentialProviderCredentialEvents*, UINT_PTR);
    bool WatchAvailable() const;
    void RefreshStatus();

private:
    LONG _ref = 1;
    CREDENTIAL_PROVIDER_USAGE_SCENARIO _scenario = CPUS_INVALID;
    std::vector<WristKeyProviderCredential*> _credentials;
    ICredentialProviderEvents* _events = nullptr;
    UINT_PTR _adviseContext = 0;
};

namespace WristKeyFields {
    enum : DWORD {
        TileImage = 0,
        Tile = 1,
        Status = 2,
        Submit = 3,
        Count = 4
    };
}
