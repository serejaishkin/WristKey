using System;
using System.IO;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Text;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;

namespace WristKeyCredentialProvider
{
    // Interface GUIDs are the REAL Windows IIDs from credentialprovider.idl
    // (Windows SDK). They must never be changed or invented: LogonUI locates
    // our implementation by QueryInterface-ing exactly these IIDs. A previous
    // version shipped fabricated GUIDs here, which made the lock-screen tile
    // impossible to load -- the root cause of the "tile never appears" bug.
    internal static class HResult
    {
        public const int S_OK = 0;
        public const int E_NOTIMPL = unchecked((int)0x80004001);
        public const int E_INVALIDARG = unchecked((int)0x80070057);
        public const int E_FAIL = unchecked((int)0x80004005);
    }

    public enum CREDENTIAL_PROVIDER_USAGE_SCENARIO { CPUS_LOGON = 1, CPUS_UNLOCK_WORKSTATION = 2 }
    public enum CREDENTIAL_PROVIDER_FIELD_TYPE : uint { CPFT_LARGE_TEXT = 1, CPFT_SMALL_TEXT = 2 }
    public enum CREDENTIAL_PROVIDER_FIELD_STATE { CPFS_HIDDEN = 0, CPFS_DISPLAY_IN_SELECTED_TILE = 1, CPFS_DISPLAY_IN_DESELECTED_TILE = 2, CPFS_DISPLAY_IN_BOTH = 3 }
    public enum CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE { CPFIS_NONE = 0 }
    public enum CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE { CPGSR_NO_CREDENTIAL_NOT_FINISHED = 0, CPGSR_NO_CREDENTIAL_FINISHED = 1, CPGSR_RETURN_CREDENTIAL_FINISHED = 2 }
    public enum CREDENTIAL_PROVIDER_STATUS_ICON { CPSI_NONE = 0, CPSI_ERROR = 1, CPSI_WARNING = 2, CPSI_SUCCESS = 3 }

    [StructLayout(LayoutKind.Sequential)]
    public struct CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR
    {
        public uint dwFieldID;
        public CREDENTIAL_PROVIDER_FIELD_TYPE cpft;
        public IntPtr pszLabel;
        public Guid guidFieldType;
    }

    // Matches the real SDK struct exactly: four fields, nothing more. Extra
    // fields here would corrupt the layout LogonUI reads back.
    [StructLayout(LayoutKind.Sequential)]
    public struct CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION
    {
        public uint ulAuthenticationPackage;
        public Guid clsidCredentialProvider;
        public uint cbSerialization;
        public IntPtr rgbSerialization;
    }

    [ComImport, Guid("D27C3481-5A1C-45B2-8AAA-C20EBBE8229E"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    public interface ICredentialProvider
    {
        [PreserveSig] int SetUsageScenario(CREDENTIAL_PROVIDER_USAGE_SCENARIO cpus, uint dwFlags);
        [PreserveSig] int SetSerialization(ref CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION pcpcs);
        [PreserveSig] int Advise(ICredentialProviderEvents pcpe, UIntPtr upAdviseContext);
        [PreserveSig] int UnAdvise();
        [PreserveSig] int GetFieldDescriptorCount(out uint pdwCount);
        [PreserveSig] int GetFieldDescriptorAt(uint dwIndex, out IntPtr ppcpfd);
        [PreserveSig] int GetCredentialCount(out uint pdwCount, out uint pdwDefault, out int pbAutoLogonWithDefault);
        [PreserveSig] int GetCredentialAt(uint dwIndex, out ICredentialProviderCredential ppcpc);
    }

    // Vtable order must match credentialprovider.idl exactly:
    // Advise..GetComboBoxValueAt, SetStringValue, GetSerialization, ReportResult.
    [ComImport, Guid("63913A93-40C1-481A-818D-4072FF8C70CC"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    public interface ICredentialProviderCredential
    {
        [PreserveSig] int Advise(ICredentialProviderCredentialEvents pcpce);
        [PreserveSig] int UnAdvise();
        [PreserveSig] int SetSelected(out int pbAutoLogon);
        [PreserveSig] int SetDeselected();
        [PreserveSig] int GetFieldState(uint dwFieldId, out CREDENTIAL_PROVIDER_FIELD_STATE pcpfs, out CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE pcpfis);
        [PreserveSig] int GetStringValue(uint dwFieldId, out IntPtr ppsz);
        [PreserveSig] int GetBitmapValue(uint dwFieldId, out IntPtr phbmp);
        [PreserveSig] int GetCheckboxValue(uint dwFieldId, out int pbChecked, out IntPtr ppszLabel);
        [PreserveSig] int GetSubmitButtonValue(uint dwFieldId, out uint pdwAdjacentTo);
        [PreserveSig] int GetComboBoxValueCount(uint dwFieldId, out uint pcItems, out uint pdwSelectedItem);
        [PreserveSig] int GetComboBoxValueAt(uint dwFieldId, uint dwItem, out IntPtr ppszItem);
        [PreserveSig] int SetStringValue(uint dwFieldId, [MarshalAs(UnmanagedType.LPWStr)] string psz);
        [PreserveSig] int GetSerialization(out CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE pcpgsr,
            out CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION pcpcs, out IntPtr ppszOptionalStatusText,
            out CREDENTIAL_PROVIDER_STATUS_ICON pcpsiOptionalStatusIcon);
        [PreserveSig] int ReportResult(int ntsStatus, int ntsSubstatus, out IntPtr ppszOptionalStatusText, out CREDENTIAL_PROVIDER_STATUS_ICON pcpsiOptionalStatusIcon);
    }

    [ComImport, Guid("34201E5A-A787-41A3-A5A4-BD6DCF2A854E"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    public interface ICredentialProviderEvents
    {
        [PreserveSig] int CredentialsChanged(UIntPtr upAdviseContext);
    }

    [ComImport, Guid("BE089DE6-CF2E-4A43-AC96-6C38D8FD34D8"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    public interface ICredentialProviderCredentialEvents
    {
        [PreserveSig] int SetFieldState(IntPtr pcpc, uint dwFieldId, CREDENTIAL_PROVIDER_FIELD_STATE cpfs);
        [PreserveSig] int SetFieldInteractiveState(IntPtr pcpc, uint dwFieldId, CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE cpfis);
        [PreserveSig] int SetFieldString(IntPtr pcpc, uint dwFieldId, [MarshalAs(UnmanagedType.LPWStr)] string psz);
        [PreserveSig] int SetFieldCheckbox(IntPtr pcpc, uint dwFieldId, int bChecked, [MarshalAs(UnmanagedType.LPWStr)] string pszLabel);
        [PreserveSig] int SetFieldSubmitButton(IntPtr pcpc, uint dwFieldId, uint dwAdjacentTo);
        [PreserveSig] int SetFieldBitmap(IntPtr pcpc, uint dwFieldId, IntPtr hbmp);
    }

    public static class NativeMethods
    {
        public const int STATUS_SUCCESS = 0;
        public const uint KerbWorkstationUnlockLogon = 7;

        [DllImport("secur32.dll", CharSet = CharSet.Ansi)]
        public static extern int LsaConnectUntrusted(out IntPtr LsaHandle);

        [DllImport("secur32.dll", CharSet = CharSet.Ansi)]
        public static extern int LsaLookupAuthenticationPackage(IntPtr LsaHandle, ref LSA_STRING PackageName, out uint AuthenticationPackage);

        [DllImport("secur32.dll")]
        public static extern int LsaDeregisterLogonProcess(IntPtr LsaHandle);

        [StructLayout(LayoutKind.Sequential)]
        public struct LSA_STRING
        {
            public ushort Length;
            public ushort MaximumLength;
            public IntPtr Buffer;
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct LUID
        {
            public uint LowPart;
            public int HighPart;
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct UNICODE_STRING
        {
            public ushort Length;
            public ushort MaximumLength;
            public IntPtr Buffer;
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct KERB_INTERACTIVE_LOGON
        {
            public uint MessageType;
            public UNICODE_STRING UserName;
            public UNICODE_STRING Domain;
            public UNICODE_STRING Password;
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct KERB_INTERACTIVE_UNLOCK_LOGON
        {
            public KERB_INTERACTIVE_LOGON Logon;
            public LUID LogonId;
        }
    }

    public class UnlockResponse
    {
        public string status { get; set; }
        public string password { get; set; }
        public string message { get; set; }
    }

    [ComVisible(true)]
    [Guid("A1B2C3D4-E5F6-7890-ABCD-EF1234567895")]
    [ClassInterface(ClassInterfaceType.None)]
    [ProgId("WristKey.CredentialProvider")]
    public class WristKeyCredentialProvider : ICredentialProvider
    {
        private ICredentialProviderEvents _events;
        private UIntPtr _adviseContext;

        public int SetUsageScenario(CREDENTIAL_PROVIDER_USAGE_SCENARIO cpus, uint dwFlags)
        {
            return cpus == CREDENTIAL_PROVIDER_USAGE_SCENARIO.CPUS_LOGON ||
                   cpus == CREDENTIAL_PROVIDER_USAGE_SCENARIO.CPUS_UNLOCK_WORKSTATION
                ? HResult.S_OK : HResult.E_NOTIMPL;
        }

        public int SetSerialization(ref CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION pcpcs)
        {
            return HResult.S_OK;
        }

        public int Advise(ICredentialProviderEvents pcpe, UIntPtr upAdviseContext)
        {
            _events = pcpe;
            _adviseContext = upAdviseContext;
            return HResult.S_OK;
        }

        public int UnAdvise()
        {
            _events = null;
            return HResult.S_OK;
        }

        public int GetFieldDescriptorCount(out uint pdwCount)
        {
            pdwCount = 2;
            return HResult.S_OK;
        }

        // LogonUI reads the allocated descriptor AND frees both the struct and
        // pszLabel with CoTaskMemFree, so both allocations must be CoTaskMem.
        public int GetFieldDescriptorAt(uint dwIndex, out IntPtr ppcpfd)
        {
            ppcpfd = IntPtr.Zero;
            if (dwIndex > 1) return HResult.E_INVALIDARG;
            var fd = new CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR
            {
                dwFieldID = dwIndex,
                cpft = dwIndex == 0 ? CREDENTIAL_PROVIDER_FIELD_TYPE.CPFT_LARGE_TEXT : CREDENTIAL_PROVIDER_FIELD_TYPE.CPFT_SMALL_TEXT,
                pszLabel = Marshal.StringToCoTaskMemUni(dwIndex == 0 ? "WristKey" : "Bring your watch close"),
                guidFieldType = Guid.Empty
            };
            ppcpfd = Marshal.AllocCoTaskMem(Marshal.SizeOf(typeof(CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR)));
            Marshal.StructureToPtr(fd, ppcpfd, false);
            return HResult.S_OK;
        }

        public int GetCredentialCount(out uint pdwCount, out uint pdwDefault, out int pbAutoLogonWithDefault)
        {
            pdwCount = 1;
            pdwDefault = unchecked((uint)-1);
            pbAutoLogonWithDefault = 0;
            return HResult.S_OK;
        }

        public int GetCredentialAt(uint dwIndex, out ICredentialProviderCredential ppcpc)
        {
            if (dwIndex != 0) { ppcpc = null; return HResult.E_INVALIDARG; }
            ppcpc = new WristKeyCredential();
            return HResult.S_OK;
        }

        internal void NotifyCredentialsChanged()
        {
            try { _events?.CredentialsChanged(_adviseContext); } catch { }
        }
    }

    [ComVisible(true)]
    [ClassInterface(ClassInterfaceType.None)]
    public class WristKeyCredential : ICredentialProviderCredential
    {
        private ICredentialProviderCredentialEvents _events;
        private string _status = "Bring your watch close";

        public int Advise(ICredentialProviderCredentialEvents pcpce)
        {
            _events = pcpce;
            return HResult.S_OK;
        }

        public int UnAdvise()
        {
            _events = null;
            return HResult.S_OK;
        }

        public int SetSelected(out int pbAutoLogon)
        {
            pbAutoLogon = 0;
            UpdateStatus("Waiting for watch confirmation...");
            return HResult.S_OK;
        }

        public int SetDeselected()
        {
            return HResult.S_OK;
        }

        public int GetFieldState(uint dwFieldId, out CREDENTIAL_PROVIDER_FIELD_STATE pcpfs, out CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE pcpfis)
        {
            if (dwFieldId > 1) { pcpfs = CREDENTIAL_PROVIDER_FIELD_STATE.CPFS_HIDDEN; pcpfis = CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE.CPFIS_NONE; return HResult.E_INVALIDARG; }
            pcpfs = CREDENTIAL_PROVIDER_FIELD_STATE.CPFS_DISPLAY_IN_SELECTED_TILE;
            pcpfis = CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE.CPFIS_NONE;
            return HResult.S_OK;
        }

        public int GetStringValue(uint dwFieldId, out IntPtr ppsz)
        {
            if (dwFieldId > 1) { ppsz = IntPtr.Zero; return HResult.E_INVALIDARG; }
            ppsz = Marshal.StringToCoTaskMemUni(dwFieldId == 0 ? "WristKey Unlock" : _status);
            return HResult.S_OK;
        }

        public int GetBitmapValue(uint dwFieldId, out IntPtr phbmp) { phbmp = IntPtr.Zero; return HResult.E_NOTIMPL; }
        public int GetCheckboxValue(uint dwFieldId, out int pbChecked, out IntPtr ppszLabel) { pbChecked = 0; ppszLabel = IntPtr.Zero; return HResult.E_NOTIMPL; }
        public int GetSubmitButtonValue(uint dwFieldId, out uint pdwAdjacentTo) { pdwAdjacentTo = 0; return HResult.E_NOTIMPL; }
        public int GetComboBoxValueCount(uint dwFieldId, out uint pcItems, out uint pdwSelectedItem) { pcItems = 0; pdwSelectedItem = 0; return HResult.E_NOTIMPL; }
        public int GetComboBoxValueAt(uint dwFieldId, uint dwItem, out IntPtr ppszItem) { ppszItem = IntPtr.Zero; return HResult.E_NOTIMPL; }

        public int SetStringValue(uint dwFieldId, string psz)
        {
            return HResult.S_OK;
        }

        private void UpdateStatus(string text)
        {
            _status = text;
            try { _events?.SetFieldString(IntPtr.Zero, 1, text); } catch { }
        }

        public int GetSerialization(out CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE pcpgsr,
            out CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION pcpcs, out IntPtr ppszOptionalStatusText,
            out CREDENTIAL_PROVIDER_STATUS_ICON pcpsiOptionalStatusIcon)
        {
            pcpgsr = CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE.CPGSR_NO_CREDENTIAL_NOT_FINISHED;
            pcpcs = new CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION();
            ppszOptionalStatusText = IntPtr.Zero;
            pcpsiOptionalStatusIcon = CREDENTIAL_PROVIDER_STATUS_ICON.CPSI_NONE;

            try
            {
                UpdateStatus("Confirming with watch...");
                string password = GetPasswordFromDaemon();
                if (string.IsNullOrEmpty(password))
                {
                    UpdateStatus("Watch did not authorize unlock");
                    ppszOptionalStatusText = Marshal.StringToCoTaskMemUni("WristKey: watch did not authorize unlock");
                    pcpsiOptionalStatusIcon = CREDENTIAL_PROVIDER_STATUS_ICON.CPSI_WARNING;
                    return HResult.S_OK;
                }

                uint authPackage = GetAuthenticationPackage();
                byte[] serialized = SerializeKerbInteractiveUnlockLogon("", password, "");

                IntPtr pSerialized = Marshal.AllocCoTaskMem(serialized.Length);
                Marshal.Copy(serialized, 0, pSerialized, serialized.Length);

                pcpcs.ulAuthenticationPackage = authPackage;
                pcpcs.clsidCredentialProvider = new Guid("A1B2C3D4-E5F6-7890-ABCD-EF1234567895");
                pcpcs.rgbSerialization = pSerialized;
                pcpcs.cbSerialization = (uint)serialized.Length;

                pcpgsr = CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE.CPGSR_RETURN_CREDENTIAL_FINISHED;
                return HResult.S_OK;
            }
            catch (Exception ex)
            {
                UpdateStatus("WristKey error: " + ex.Message);
                ppszOptionalStatusText = Marshal.StringToCoTaskMemUni("WristKey error: " + ex.Message);
                pcpsiOptionalStatusIcon = CREDENTIAL_PROVIDER_STATUS_ICON.CPSI_ERROR;
                pcpgsr = CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE.CPGSR_NO_CREDENTIAL_NOT_FINISHED;
                return HResult.S_OK;
            }
        }

        public int ReportResult(int ntsStatus, int ntsSubstatus, out IntPtr ppszOptionalStatusText, out CREDENTIAL_PROVIDER_STATUS_ICON pcpsiOptionalStatusIcon)
        {
            ppszOptionalStatusText = IntPtr.Zero;
            pcpsiOptionalStatusIcon = CREDENTIAL_PROVIDER_STATUS_ICON.CPSI_NONE;
            if (ntsStatus != 0)
            {
                UpdateStatus("Windows rejected the credential");
                ppszOptionalStatusText = Marshal.StringToCoTaskMemUni("WristKey: Windows rejected the credential");
                pcpsiOptionalStatusIcon = CREDENTIAL_PROVIDER_STATUS_ICON.CPSI_ERROR;
            }
            return HResult.S_OK;
        }

        private string GetPasswordFromDaemon()
        {
            try
            {
                using (NamedPipeClientStream client = new NamedPipeClientStream(".", "wristkey",
                    PipeDirection.InOut))
                {
                    client.Connect(15000);
                    using (StreamWriter writer = new StreamWriter(client, Encoding.UTF8) { AutoFlush = true })
                    {
                        var request = new JObject();
                        request["action"] = "unlock";
                        request["user"] = Environment.UserName;
                        // WriteLine adds \r\n which the daemon's read_line handles,
                        // but keep explicit \n for consistency.
                        writer.Write(request.ToString(Newtonsoft.Json.Formatting.None) + "\n");
                        writer.Flush();
                    }
                    using (StreamReader reader = new StreamReader(client, Encoding.UTF8))
                    {
                        string responseJson = reader.ReadLine();
                        if (string.IsNullOrEmpty(responseJson))
                            throw new Exception("Empty response from daemon");
                        var response = JsonConvert.DeserializeObject<UnlockResponse>(responseJson);
                        if (response == null)
                            throw new Exception("Failed to parse daemon response");
                        if (response.status == "success")
                        {
                            return response.password;
                        }
                        throw new Exception(response.message ?? "Unknown error from daemon");
                    }
                }
            }
            catch (Exception ex)
            {
                throw new Exception("Failed to reach WristKey daemon: " + ex.Message);
            }
        }

        private uint GetAuthenticationPackage()
        {
            IntPtr lsaHandle;
            if (NativeMethods.LsaConnectUntrusted(out lsaHandle) != NativeMethods.STATUS_SUCCESS)
                return 2;

            string name = "kerberos";
            NativeMethods.LSA_STRING packageName = new NativeMethods.LSA_STRING
            {
                Length = (ushort)name.Length,
                MaximumLength = (ushort)(name.Length + 1),
                Buffer = Marshal.StringToHGlobalAnsi(name)
            };

            uint authPackage;
            int result = NativeMethods.LsaLookupAuthenticationPackage(lsaHandle, ref packageName, out authPackage);

            Marshal.FreeHGlobal(packageName.Buffer);
            NativeMethods.LsaDeregisterLogonProcess(lsaHandle);

            return result == NativeMethods.STATUS_SUCCESS ? authPackage : 2;
        }

        // Serializes KERB_INTERACTIVE_UNLOCK_LOGON for Kerberos.
        //
        // KNOWN LIMITATION: LogonId is left zeroed. Some Windows builds accept
        // this for CPUS_UNLOCK_WORKSTATION, some reject the resulting
        // credential. If logon fails after a successful watch confirmation,
        // the LogonId of the session being unlocked must be resolved here
        // (e.g. via ICredentialProviderSetUserArray / LsaEnumerateLogonSessions).
        private byte[] SerializeKerbInteractiveUnlockLogon(string username, string password, string domain)
        {
            NativeMethods.KERB_INTERACTIVE_UNLOCK_LOGON logon = new NativeMethods.KERB_INTERACTIVE_UNLOCK_LOGON
            {
                Logon = new NativeMethods.KERB_INTERACTIVE_LOGON
                {
                    MessageType = NativeMethods.KerbWorkstationUnlockLogon,
                }
            };

            IntPtr pUserName = username.Length == 0 ? IntPtr.Zero : Marshal.StringToHGlobalUni(username);
            IntPtr pDomain = domain.Length == 0 ? IntPtr.Zero : Marshal.StringToHGlobalUni(domain);
            IntPtr pPassword = password.Length == 0 ? IntPtr.Zero : Marshal.StringToHGlobalUni(password);

            try
            {
                logon.Logon.UserName = new NativeMethods.UNICODE_STRING
                {
                    Length = (ushort)(username.Length * 2),
                    MaximumLength = (ushort)((username.Length + 1) * 2),
                    Buffer = pUserName
                };
                logon.Logon.Domain = new NativeMethods.UNICODE_STRING
                {
                    Length = (ushort)(domain.Length * 2),
                    MaximumLength = (ushort)((domain.Length + 1) * 2),
                    Buffer = pDomain
                };
                logon.Logon.Password = new NativeMethods.UNICODE_STRING
                {
                    Length = (ushort)(password.Length * 2),
                    MaximumLength = (ushort)((password.Length + 1) * 2),
                    Buffer = pPassword
                };

                int size = Marshal.SizeOf(typeof(NativeMethods.KERB_INTERACTIVE_UNLOCK_LOGON));
                IntPtr pLogon = Marshal.AllocCoTaskMem(size);
                Marshal.StructureToPtr(logon, pLogon, false);

                byte[] result = new byte[size];
                Marshal.Copy(pLogon, result, 0, size);
                Marshal.FreeCoTaskMem(pLogon);
                return result;
            }
            finally
            {
                if (pUserName != IntPtr.Zero) Marshal.FreeHGlobal(pUserName);
                if (pDomain != IntPtr.Zero) Marshal.FreeHGlobal(pDomain);
                if (pPassword != IntPtr.Zero) Marshal.FreeHGlobal(pPassword);
            }
        }
    }
}
