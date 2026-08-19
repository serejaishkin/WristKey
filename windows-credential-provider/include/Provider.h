#pragma once
#include <windows.h>
#include <credentialprovider.h>
#include <string>
#include <vector>

class WristKeyProvider;

class WristKeyProviderCredential final : public ICredentialProviderCredential {
public:
    explicit WristKeyProviderCredential(WristKeyProvider* provider);
    ~WristKeyProviderCredential() override = default;

    // IUnknown
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID riid, void** ppv) override;
    ULONG STDMETHODCALLTYPE AddRef() override;
    ULONG STDMETHODCALLTYPE Release() override;

    // ICredentialProviderCredential
    HRESULT STDMETHODCALLTYPE Advise(ICredentialProviderCredentialEvents* pcpce) override;
    HRESULT STDMETHODCALLTYPE UnAdvise() override;
    HRESULT STDMETHODCALLTYPE SetSelected(BOOL* pbAutoLogon) override;
    HRESULT STDMETHODCALLTYPE SetDeselected() override;
    HRESULT STDMETHODCALLTYPE GetFieldState(DWORD dwFieldID, CREDENTIAL_PROVIDER_FIELD_STATE* pcpfs,
                                             CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE* pcpfis) override;
    HRESULT STDMETHODCALLTYPE GetStringValue(DWORD dwFieldID, PWSTR* ppwsz) override;
    HRESULT STDMETHODCALLTYPE GetBitmapValue(DWORD dwFieldID, HBITMAP* phbmp) override;
    HRESULT STDMETHODCALLTYPE GetCheckboxValue(DWORD dwFieldID, BOOL* pbChecked) override;
    HRESULT STDMETHODCALLTYPE GetComboBoxValueCount(DWORD dwFieldID, DWORD* pdwCount) override;
    HRESULT STDMETHODCALLTYPE GetComboBoxValueAt(DWORD dwFieldID, DWORD dwItem,
                                                  PWSTR* ppwszItem) override;
    HRESULT STDMETHODCALLTYPE GetSubmitButtonValue(DWORD dwFieldID, DWORD* pdwAdjacentTo) override;
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

private:
    LONG _ref = 1;
    WristKeyProvider* _provider = nullptr;
    ICredentialProviderCredentialEvents* _events = nullptr;
};

class WristKeyProvider final : public ICredentialProvider {
public:
    WristKeyProvider();
    ~WristKeyProvider() override;

    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID riid, void** ppv) override;
    ULONG STDMETHODCALLTYPE AddRef() override;
    ULONG STDMETHODCALLTYPE Release() override;
    HRESULT STDMETHODCALLTYPE SetUsageScenario(CREDENTIAL_PROVIDER_USAGE_SCENARIO cpus, DWORD dwFlags) override;
    HRESULT STDMETHODCALLTYPE SetSerialization(const CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION* pcpcs) override;
    HRESULT STDMETHODCALLTYPE Advise(ICredentialProviderEvents* pcpe, UINT_PTR upAdviseContext) override;
    HRESULT STDMETHODCALLTYPE UnAdvise() override;
    HRESULT STDMETHODCALLTYPE GetFieldDescriptorCount(DWORD* pdwCount) override;
    HRESULT STDMETHODCALLTYPE GetFieldDescriptorAt(DWORD dwIndex, ICredentialProviderFieldDescriptor** ppcpfd) override;
    HRESULT STDMETHODCALLTYPE GetCredentialCount(DWORD* pdwCount, DWORD* pdwDefault, BOOL* pbAutoLogonWithDefault) override;
    HRESULT STDMETHODCALLTYPE GetCredentialAt(DWORD dwIndex, ICredentialProviderCredential** ppcpc) override;

    void SetEvents(ICredentialProviderCredentialEvents* events, UINT_PTR context);
    bool WatchAvailable() const;
    void RefreshStatus();

private:
    LONG _ref = 1;
    CREDENTIAL_PROVIDER_USAGE_SCENARIO _scenario = CPUS_INVALID;
    WristKeyProviderCredential* _credential = nullptr;
    ICredentialProviderEvents* _events = nullptr;
    UINT_PTR _adviseContext = 0;
};

namespace WristKeyFields {
    enum : DWORD {
        Tile = 0,
        Status = 1,
        Submit = 2,
        Count = 3
    };
}
