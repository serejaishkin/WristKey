using System;
using System.Runtime.InteropServices;

namespace WristKeyCredentialProvider
{
    internal static class HResult
    {
        public const int S_OK = 0;
        public const int E_NOTIMPL = unchecked((int)0x80004001);
        public const int E_INVALIDARG = unchecked((int)0x80070057);
    }

    public enum CREDENTIAL_PROVIDER_USAGE_SCENARIO : uint { CPUS_INVALID = 0, CPUS_LOGON = 1, CPUS_UNLOCK_WORKSTATION = 2, CPUS_CHANGE_PASSWORD = 3, CPUS_CREDUI = 4, CPUS_PLAP = 5 }
    public enum CREDENTIAL_PROVIDER_FIELD_TYPE : uint { CPFT_INVALID = 0, CPFT_LARGE_TEXT = 1, CPFT_SMALL_TEXT = 2, CPFT_COMMAND_LINK = 3, CPFT_EDIT_TEXT = 4, CPFT_PASSWORD_TEXT = 5, CPFT_TILE_IMAGE = 6, CPFT_CHECKBOX = 7, CPFT_COMBOBOX = 8, CPFT_SUBMIT_BUTTON = 9, CPFT_VALIDATION_SMALL_TEXT = 10, CPFT_VISUAL_CUE = 11, CPFT_LABEL = 12, CPFT_BITMAP = 13, CPFT_PASSWORD_TEXT_WITH_BUTTON = 14, CPFT_CREDENTIAL_PROVIDER_LOGO = 15, CPFT_CREDENTIAL_PROVIDER_LABEL = 16 }
    public enum CREDENTIAL_PROVIDER_FIELD_STATE : uint { CPFS_HIDDEN = 0, CPFS_DISPLAY_IN_SELECTED_TILE = 1, CPFS_DISPLAY_IN_DESELECTED_TILE = 2, CPFS_DISPLAY_IN_BOTH = 3 }
    public enum CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE : uint { CPFIS_NONE = 0, CPFIS_READONLY = 1, CPFIS_DISABLED = 2, CPFIS_FOCUSED = 3 }
    public enum CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE : uint { CPGSR_NO_CREDENTIAL_NOT_FINISHED = 0, CPGSR_NO_CREDENTIAL_FINISHED = 1, CPGSR_RETURN_CREDENTIAL_FINISHED = 2 }
    public enum CREDENTIAL_PROVIDER_STATUS_ICON : uint { CPSI_NONE = 0, CPSI_ERROR = 1, CPSI_WARNING = 2, CPSI_SUCCESS = 3 }

    [StructLayout(LayoutKind.Sequential)]
    public struct CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR
    {
        public uint dwFieldID;
        public CREDENTIAL_PROVIDER_FIELD_TYPE cpft;
        public IntPtr pszLabel;
        public Guid guidFieldType;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION
    {
        public uint ulAuthenticationPackage;
        public Guid clsidCredentialProvider;
        public uint cbSerialization;
        public IntPtr rgbSerialization;
    }

    // Native Windows ICredentialProvider vtable. GUIDs and method order match
    // credentialprovider.h; do not reorder these methods.
    [ComVisible(true)]
    [Guid("D27C3481-5A1C-45B2-8AAA-C20EBBE8229E")]
    [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    public interface ICredentialProvider
    {
        int SetUsageScenario(CREDENTIAL_PROVIDER_USAGE_SCENARIO cpus, uint dwFlags);
        int SetSerialization(IntPtr pcpcs);
        int Advise(ICredentialProviderEvents pcpe, UIntPtr upAdviseContext);
        int UnAdvise();
        int GetFieldDescriptorCount(out uint pdwCount);
        int GetFieldDescriptorAt(uint dwIndex, out IntPtr ppcpfd);
        int GetCredentialCount(out uint pdwCount, out uint pdwDefault, out int pbAutoLogonWithDefault);
        int GetCredentialAt(uint dwIndex, out ICredentialProviderCredential ppcpc);
    }

    [ComVisible(true)]
    [Guid("63913A93-40C1-481A-818D-4072FF8C70CC")]
    [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    public interface ICredentialProviderCredential
    {
        int Advise(ICredentialProviderCredentialEvents pcpce);
        int UnAdvise();
        int SetSelected(out int pbAutoLogon);
        int SetDeselected();
        int GetFieldState(uint dwFieldID, out CREDENTIAL_PROVIDER_FIELD_STATE pcpfs, out CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE pcpfis);
        int GetStringValue(uint dwFieldID, out IntPtr ppsz);
        int GetBitmapValue(uint dwFieldID, out IntPtr phbmp);
        int GetCheckboxValue(uint dwFieldID, out int pbChecked, out IntPtr ppszLabel);
        int GetSubmitButtonValue(uint dwFieldID, out uint pdwAdjacentTo);
        int GetComboBoxValueCount(uint dwFieldID, out uint pcItems, out uint pdwSelectedItem);
        int GetComboBoxValueAt(uint dwFieldID, uint dwItem, out IntPtr ppszItem);
        int SetFieldInteractiveState(ICredentialProviderCredential pcpc, uint dwFieldID, CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE cpfis);
        int SetFieldString(ICredentialProviderCredential pcpc, uint dwFieldID, string psz);
        int SetFieldCheckbox(ICredentialProviderCredential pcpc, uint dwFieldID, int bChecked, string pszLabel);
        int SetFieldBitmap(ICredentialProviderCredential pcpc, uint dwFieldID, IntPtr hbmp);
        int SetFieldComboBoxSelectedItem(ICredentialProviderCredential pcpc, uint dwFieldID, uint dwSelectedItem);
        int DeleteFieldComboBoxItem(ICredentialProviderCredential pcpc, uint dwFieldID, uint dwItem);
        int AppendFieldComboBoxItem(ICredentialProviderCredential pcpc, uint dwFieldID, string pszItem);
        int SetFieldSubmitButton(ICredentialProviderCredential pcpc, uint dwFieldID, uint dwAdjacentTo);
        int OnCreatingWindow(out IntPtr phwndOwner);
        int GetSerialization(out CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE pcpgsr, out CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION pcpcs, out IntPtr ppszOptionalStatusText, out CREDENTIAL_PROVIDER_STATUS_ICON pcpsiOptionalStatusIcon);
        int ReportResult(int ntsStatus, int ntsSubstatus, out IntPtr ppszOptionalStatusText, out CREDENTIAL_PROVIDER_STATUS_ICON pcpsiOptionalStatusIcon);
    }

    [ComVisible(true)]
    [Guid("34201E5A-A787-41A3-A5A4-BD6DCF2A854E")]
    [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    public interface ICredentialProviderEvents { int CredentialsChanged(UIntPtr upAdviseContext); }

    [ComVisible(true)]
    [Guid("FA6FA76B-66B7-4B11-95F1-86171118E816")]
    [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    public interface ICredentialProviderCredentialEvents
    {
        int SetFieldState(ICredentialProviderCredential pcpc, uint dwFieldID, CREDENTIAL_PROVIDER_FIELD_STATE cpfs);
        int SetFieldInteractiveState(ICredentialProviderCredential pcpc, uint dwFieldID, CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE cpfis);
        int SetFieldString(ICredentialProviderCredential pcpc, uint dwFieldID, string psz);
        int SetFieldCheckbox(ICredentialProviderCredential pcpc, uint dwFieldID, int bChecked, string pszLabel);
        int SetFieldBitmap(ICredentialProviderCredential pcpc, uint dwFieldID, IntPtr hbmp);
        int SetFieldComboBoxSelectedItem(ICredentialProviderCredential pcpc, uint dwFieldID, uint dwSelectedItem);
        int DeleteFieldComboBoxItem(ICredentialProviderCredential pcpc, uint dwFieldID, uint dwItem);
        int AppendFieldComboBoxItem(ICredentialProviderCredential pcpc, uint dwFieldID, string pszItem);
        int SetFieldSubmitButton(ICredentialProviderCredential pcpc, uint dwFieldID, uint dwAdjacentTo);
    }

    [ComVisible(true)]
    [Guid("A1B2C3D4-E5F6-7890-ABCD-EF1234567895")]
    [ProgId("WristKey.CredentialProvider")]
    [ClassInterface(ClassInterfaceType.None)]
    public sealed class WristKeyCredentialProvider : ICredentialProvider
    {
        public static readonly Guid ProviderClsid = new Guid("A1B2C3D4-E5F6-7890-ABCD-EF1234567895");
        public int SetUsageScenario(CREDENTIAL_PROVIDER_USAGE_SCENARIO cpus, uint dwFlags) =>
            cpus == CREDENTIAL_PROVIDER_USAGE_SCENARIO.CPUS_LOGON || cpus == CREDENTIAL_PROVIDER_USAGE_SCENARIO.CPUS_UNLOCK_WORKSTATION ? HResult.S_OK : HResult.E_NOTIMPL;
        public int SetSerialization(IntPtr pcpcs) => HResult.E_NOTIMPL;
        public int Advise(ICredentialProviderEvents pcpe, UIntPtr upAdviseContext) => HResult.S_OK;
        public int UnAdvise() => HResult.S_OK;
        public int GetFieldDescriptorCount(out uint pdwCount) { pdwCount = 2; return HResult.S_OK; }
        public int GetFieldDescriptorAt(uint dwIndex, out IntPtr ppcpfd)
        {
            ppcpfd = IntPtr.Zero;
            if (dwIndex > 1) return HResult.E_INVALIDARG;
            var fd = new CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR { dwFieldID = dwIndex, cpft = dwIndex == 0 ? CREDENTIAL_PROVIDER_FIELD_TYPE.CPFT_LARGE_TEXT : CREDENTIAL_PROVIDER_FIELD_TYPE.CPFT_SMALL_TEXT, pszLabel = Marshal.StringToCoTaskMemUni(dwIndex == 0 ? "WristKey" : "Status"), guidFieldType = Guid.Empty };
            ppcpfd = Marshal.AllocCoTaskMem(Marshal.SizeOf(typeof(CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR)));
            Marshal.StructureToPtr(fd, ppcpfd, false);
            return HResult.S_OK;
        }
        public int GetCredentialCount(out uint pdwCount, out uint pdwDefault, out int pbAutoLogonWithDefault) { pdwCount = 1; pdwDefault = unchecked((uint)-1); pbAutoLogonWithDefault = 0; return HResult.S_OK; }
        public int GetCredentialAt(uint dwIndex, out ICredentialProviderCredential ppcpc) { if (dwIndex != 0) { ppcpc = null; return HResult.E_INVALIDARG; } ppcpc = new WristKeyCredential(); return HResult.S_OK; }
    }

    [ComVisible(true)]
    [ClassInterface(ClassInterfaceType.None)]
    public sealed class WristKeyCredential : ICredentialProviderCredential
    {
        public int Advise(ICredentialProviderCredentialEvents pcpce) => HResult.S_OK;
        public int UnAdvise() => HResult.S_OK;
        public int SetSelected(out int pbAutoLogon) { pbAutoLogon = 0; return HResult.S_OK; }
        public int SetDeselected() => HResult.S_OK;
        public int GetFieldState(uint dwFieldID, out CREDENTIAL_PROVIDER_FIELD_STATE pcpfs, out CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE pcpfis) { pcpfs = dwFieldID <= 1 ? CREDENTIAL_PROVIDER_FIELD_STATE.CPFS_DISPLAY_IN_BOTH : CREDENTIAL_PROVIDER_FIELD_STATE.CPFS_HIDDEN; pcpfis = CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE.CPFIS_NONE; return dwFieldID <= 1 ? HResult.S_OK : HResult.E_INVALIDARG; }
        public int GetStringValue(uint dwFieldID, out IntPtr ppsz) { ppsz = Marshal.StringToCoTaskMemUni(dwFieldID == 0 ? "WristKey" : "Bring your watch close"); return dwFieldID <= 1 ? HResult.S_OK : HResult.E_INVALIDARG; }
        public int GetBitmapValue(uint dwFieldID, out IntPtr phbmp) { phbmp = IntPtr.Zero; return HResult.E_NOTIMPL; }
        public int GetCheckboxValue(uint dwFieldID, out int pbChecked, out IntPtr ppszLabel) { pbChecked = 0; ppszLabel = IntPtr.Zero; return HResult.E_NOTIMPL; }
        public int GetSubmitButtonValue(uint dwFieldID, out uint pdwAdjacentTo) { pdwAdjacentTo = 0; return HResult.E_NOTIMPL; }
        public int GetComboBoxValueCount(uint dwFieldID, out uint pcItems, out uint pdwSelectedItem) { pcItems = 0; pdwSelectedItem = 0; return HResult.E_NOTIMPL; }
        public int GetComboBoxValueAt(uint dwFieldID, uint dwItem, out IntPtr ppszItem) { ppszItem = IntPtr.Zero; return HResult.E_NOTIMPL; }
        public int SetFieldInteractiveState(ICredentialProviderCredential pcpc, uint dwFieldID, CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE cpfis) => HResult.E_NOTIMPL;
        public int SetFieldString(ICredentialProviderCredential pcpc, uint dwFieldID, string psz) => HResult.E_NOTIMPL;
        public int SetFieldCheckbox(ICredentialProviderCredential pcpc, uint dwFieldID, int bChecked, string pszLabel) => HResult.E_NOTIMPL;
        public int SetFieldBitmap(ICredentialProviderCredential pcpc, uint dwFieldID, IntPtr hbmp) => HResult.E_NOTIMPL;
        public int SetFieldComboBoxSelectedItem(ICredentialProviderCredential pcpc, uint dwFieldID, uint dwSelectedItem) => HResult.E_NOTIMPL;
        public int DeleteFieldComboBoxItem(ICredentialProviderCredential pcpc, uint dwFieldID, uint dwItem) => HResult.E_NOTIMPL;
        public int AppendFieldComboBoxItem(ICredentialProviderCredential pcpc, uint dwFieldID, string pszItem) => HResult.E_NOTIMPL;
        public int SetFieldSubmitButton(ICredentialProviderCredential pcpc, uint dwFieldID, uint dwAdjacentTo) => HResult.E_NOTIMPL;
        public int OnCreatingWindow(out IntPtr phwndOwner) { phwndOwner = IntPtr.Zero; return HResult.S_OK; }

        // Deliberately non-authenticating in this first phase. This makes the
        // tile safe to test before wiring the WristKey challenge into Winlogon.
        public int GetSerialization(out CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE pcpgsr, out CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION pcpcs, out IntPtr ppszOptionalStatusText, out CREDENTIAL_PROVIDER_STATUS_ICON pcpsiOptionalStatusIcon)
        {
            pcpgsr = CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE.CPGSR_NO_CREDENTIAL_NOT_FINISHED;
            pcpcs = new CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION();
            ppszOptionalStatusText = Marshal.StringToCoTaskMemUni("WristKey is waiting for the watch");
            pcpsiOptionalStatusIcon = CREDENTIAL_PROVIDER_STATUS_ICON.CPSI_NONE;
            return HResult.S_OK;
        }
        public int ReportResult(int ntsStatus, int ntsSubstatus, out IntPtr ppszOptionalStatusText, out CREDENTIAL_PROVIDER_STATUS_ICON pcpsiOptionalStatusIcon) { ppszOptionalStatusText = IntPtr.Zero; pcpsiOptionalStatusIcon = CREDENTIAL_PROVIDER_STATUS_ICON.CPSI_NONE; return HResult.S_OK; }
    }
}
