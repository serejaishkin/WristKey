#pragma once

#include <windows.h>
#include <credentialprovider.h>
#include <string>
#include <vector>

class WristKeyProviderCredential;

class WristKeyProvider final : public ICredentialProvider, public ICredentialProviderSetUserArray {
public:
    WristKeyProvider();
    ~WristKeyProvider();

    // IUnknown
    STDMETHOD(QueryInterface)(REFIID riid, void** ppv);
    STDMETHOD_(ULONG, AddRef)();
    STDMETHOD_(ULONG, Release)();

    // ICredentialProvider
    STDMETHOD(SetUsageScenario)(CREDENTIAL_PROVIDER_USAGE_SCENARIO cpus, DWORD dwFlags);
    STDMETHOD(SetSerialization)(const CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION* pcpcs);
    STDMETHOD(Advise)(ICredentialProviderEvents* pcpe, UINT_PTR upAdviseContext);
    STDMETHOD(UnAdvise)();
    STDMETHOD(GetFieldDescriptorCount)(DWORD* pdwCount);
    STDMETHOD(GetFieldDescriptorAt)(DWORD dwIndex, CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR** ppcpfd);
    STDMETHOD(GetCredentialCount)(DWORD* pdwCount, DWORD* pdwDefault, BOOL* pbAutoLogonWithDefault);
    STDMETHOD(GetCredentialAt)(DWORD dwIndex, ICredentialProviderCredential** ppcpc);
    STDMETHOD(SetUserArray)(ICredentialProviderUserArray* users);

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
    DWORD _lastAutoLogonTick = 0;
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

class WristKeyProviderCredential final : public ICredentialProviderCredential2 {
public:
    WristKeyProviderCredential(WristKeyProvider* provider,
                               CREDENTIAL_PROVIDER_USAGE_SCENARIO scenario,
                               std::wstring username,
                               std::wstring sid);
    ~WristKeyProviderCredential();

    STDMETHOD(QueryInterface)(REFIID riid, void** ppv);
    STDMETHOD_(ULONG, AddRef)();
    STDMETHOD_(ULONG, Release)();
    STDMETHOD(Advise)(ICredentialProviderCredentialEvents* pcpce);
    STDMETHOD(UnAdvise)();
    STDMETHOD(SetSelected)(BOOL* pbAutoLogon);
    STDMETHOD(SetDeselected)();
    STDMETHOD(GetFieldState)(DWORD dwFieldID, CREDENTIAL_PROVIDER_FIELD_STATE* pcpfs,
                             CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE* pcpfis);
    STDMETHOD(GetStringValue)(DWORD dwFieldID, PWSTR* ppwsz);
    STDMETHOD(GetBitmapValue)(DWORD dwFieldID, HBITMAP* phbmp);
    STDMETHOD(GetCheckboxValue)(DWORD dwFieldID, BOOL* pbChecked, LPWSTR* ppszLabel);
    STDMETHOD(GetSubmitButtonValue)(DWORD dwFieldID, DWORD* pdwAdjacentTo);
    STDMETHOD(GetComboBoxValueCount)(DWORD dwFieldID, DWORD* pcItems, DWORD* pdwSelectedItem);
    STDMETHOD(GetComboBoxValueAt)(DWORD dwFieldID, DWORD dwItem, PWSTR* ppwszItem);
    STDMETHOD(SetStringValue)(DWORD dwFieldID, PCWSTR pwz);
    STDMETHOD(SetCheckboxValue)(DWORD dwFieldID, BOOL bChecked);
    STDMETHOD(SetComboBoxSelectedValue)(DWORD dwFieldID, DWORD dwSelectedItem);
    STDMETHOD(CommandLinkClicked)(DWORD dwFieldID);
    STDMETHOD(GetSerialization)(CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE* pcpgsr,
                                CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION* pcpcs,
                                PWSTR* ppwszOptionalStatusText,
                                CREDENTIAL_PROVIDER_STATUS_ICON* pcpsiOptionalStatusIcon);
    STDMETHOD(ReportResult)(NTSTATUS ntsStatus, NTSTATUS ntsSubstatus,
                            PWSTR* ppwszOptionalStatusText,
                            CREDENTIAL_PROVIDER_STATUS_ICON* pcpsiOptionalStatusIcon);
    STDMETHOD(GetUserSid)(PWSTR* ppszSid);

private:
    LONG _ref = 1;
    WristKeyProvider* _provider = nullptr;
    CREDENTIAL_PROVIDER_USAGE_SCENARIO _scenario = CPUS_INVALID;
    ICredentialProviderCredentialEvents* _events = nullptr;
    std::wstring _username;
    std::wstring _sid;
};
