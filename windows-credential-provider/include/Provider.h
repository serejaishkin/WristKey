#pragma once

#include <windows.h>
#include <guiddef.h>
#include <credentialprovider.h>
#include <string>
#include <vector>

// Keep this provider self-contained: including ShlGuid.h pulls in the legacy
// shell automation declarations and can conflict with the SDK headers used by
// Credential Provider builds. These two GUIDs are the only ShlGuid values we
// need here.
#ifndef GUID_NULL
#define GUID_NULL GUID{0, 0, 0, {0, 0, 0, 0, 0, 0, 0, 0}}
#endif
#ifndef CPFG_CREDENTIAL_PROVIDER_LOGO
#define CPFG_CREDENTIAL_PROVIDER_LOGO GUID{0x2d837775, 0xf6cd, 0x464e, {0xa7, 0x45, 0x48, 0x2f, 0xd0, 0xb4, 0x74, 0x93}}
#endif

// {7E1B7B8A-4C8B-4C2F-9D8A-8D3A7F2E51A1}
EXTERN_C const GUID CLSID_WristKeyCredentialProvider;

class WristKeyProvider;

class WristKeyProviderCredential final : public ICredentialProviderCredential2 {
public:
    WristKeyProviderCredential(WristKeyProvider* provider, std::wstring username, std::wstring sid);
    ~WristKeyProviderCredential();

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

class WristKeyProvider final : public ICredentialProvider, public ICredentialProviderSetUserArray {
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
    HRESULT STDMETHODCALLTYPE SetUserArray(ICredentialProviderUserArray* users) override;

    void RefreshStatus();

private:
    void ReleaseCredentials();
    HRESULT CreateCredentials();

    LONG _ref = 1;
    CREDENTIAL_PROVIDER_USAGE_SCENARIO _scenario = CPUS_INVALID;
    bool _recreateCredentials = true;
    std::vector<WristKeyProviderCredential*> _credentials;
    ICredentialProviderUserArray* _userArray = nullptr;
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
