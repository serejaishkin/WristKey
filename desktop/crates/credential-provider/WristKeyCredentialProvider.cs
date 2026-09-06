using System;
using System.IO;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Runtime.Serialization.Json;
using System.Security.Principal;
using System.Text;

namespace WristKeyCredentialProvider
{
    [ComImport, Guid("D27C3481-5A1C-45B2-8AAA-C20EBBE8229E"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    public interface ICredentialProvider
    {
        [PreserveSig] int SetUsageScenario(CREDENTIAL_PROVIDER_USAGE_SCENARIO cpus, uint dwFlags);
        [PreserveSig] int SetSerialization(ref CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION pcpcs);
        [PreserveSig] int Advise(ICredentialProviderEvents pcpe, ulong upAdviseContext);
        [PreserveSig] int UnAdvise();
        [PreserveSig] int GetFieldDescriptorCount(out uint pdwCount);
        [PreserveSig] int GetFieldDescriptorAt(uint dwIndex, out IntPtr ppcpfd);
        [PreserveSig] int GetCredentialCount(out uint pdwCount, out uint pdwDefault, out int pbAutoLogonWithDefault);
        [PreserveSig] int GetCredentialAt(uint dwIndex, out ICredentialProviderCredential ppcpc);
    }

    [ComImport, Guid("63913A93-40C1-481A-818D-4072FF8C70CC"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    public interface ICredentialProviderCredential
    {
        [PreserveSig] int Advise(ICredentialProviderCredentialEvents pcpce);
        [PreserveSig] int UnAdvise();
        [PreserveSig] int SetSelected(out int pbAutoLogon);
        [PreserveSig] int SetDeselected();
        [PreserveSig] int GetFieldState(uint dwFieldId, out CREDENTIAL_PROVIDER_FIELD_STATE pcpfs, out CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE pcpfis);
        [PreserveSig] int GetStringValue(uint dwFieldId, out string ppsz);
        [PreserveSig] int GetBitmapValue(uint dwFieldId, out IntPtr phbmp);
        [PreserveSig] int GetCheckboxValue(uint dwFieldId, out int pbChecked, out string ppszLabel);
        [PreserveSig] int GetSubmitButtonValue(uint dwFieldId, out uint pdwAdjacentTo);
        [PreserveSig] int GetComboBoxValueCount(uint dwFieldId, out uint pcItems, out uint pdwSelectedItem);
        [PreserveSig] int GetComboBoxValueAt(uint dwFieldId, uint dwItem, out string ppszItem);
        [PreserveSig] int SetStringValue(uint dwFieldId, string psz);
        [PreserveSig] int SetCheckboxValue(uint dwFieldId, int bChecked);
        [PreserveSig] int SetComboBoxSelectedValue(uint dwFieldId, uint dwSelectedItem);
        [PreserveSig] int CommandLinkClicked(uint dwFieldId);
        [PreserveSig] int GetSerialization(out CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE pcpgsr,
            out CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION pcpcs, out string ppszOptionalStatusText,
            out int pcpsiOptionalStatusIcon);
        [PreserveSig] int ReportResult(int ntsStatus, int ntsSubstatus, out string ppszOptionalStatusText, out int pcpsiOptionalStatusIcon);
    }

    [ComImport, Guid("34201E5A-A787-41A3-A5A4-BD6DCF2A854E"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    public interface ICredentialProviderEvents
    {
        [PreserveSig] int CredentialsChanged(ulong upAdviseContext);
    }

    [ComImport, Guid("FA6FA76B-66B7-4B11-95F1-86171118E816"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    public interface ICredentialProviderCredentialEvents
    {
        [PreserveSig] int SetFieldState(IntPtr pcpc, uint dwFieldId, CREDENTIAL_PROVIDER_FIELD_STATE cpfs);
        [PreserveSig] int SetFieldInteractiveState(IntPtr pcpc, uint dwFieldId, CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE cpfis);
        [PreserveSig] int SetFieldString(IntPtr pcpc, uint dwFieldId, string psz);
        [PreserveSig] int SetFieldCheckbox(IntPtr pcpc, uint dwFieldId, int bChecked, string pszLabel);
        [PreserveSig] int SetFieldBitmap(IntPtr pcpc, uint dwFieldId, IntPtr hbmp);
        [PreserveSig] int SetFieldComboBoxSelectedItem(IntPtr pcpc, uint dwFieldId, uint dwSelectedItem);
        [PreserveSig] int DeleteFieldComboBoxItem(IntPtr pcpc, uint dwFieldId, uint dwItem);
        [PreserveSig] int AppendFieldComboBoxItem(IntPtr pcpc, uint dwFieldId, string pszItem);
        [PreserveSig] int SetFieldSubmitButton(IntPtr pcpc, uint dwFieldId, uint dwAdjacentTo);
        [PreserveSig] int OnCreatingWindow(out IntPtr phwndOwner);
    }

    [ComImport, Guid("095C1484-1C0C-4388-9C6D-500E61BF84BD"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    public interface ICredentialProviderSetUserArray
    {
        [PreserveSig] int SetUserArray(ICredentialProviderUserArray users);
    }

    [ComImport, Guid("90C119AE-0F18-4520-A1F1-114366A40FE8"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    public interface ICredentialProviderUserArray
    {
        [PreserveSig] int SetProviderFilter(ref Guid providerToFilterTo);
        [PreserveSig] int GetAccountOptions(out uint options);
        [PreserveSig] int GetCount(out uint userCount);
        [PreserveSig] int GetAt(uint userIndex, out ICredentialProviderUser user);
    }

    [ComImport, Guid("13793285-3EA6-40FD-B420-15F47DA41FBB"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    public interface ICredentialProviderUser
    {
        [PreserveSig] int GetSid([MarshalAs(UnmanagedType.LPWStr)] out string sid);
        [PreserveSig] int GetProviderID(out Guid providerId);
        [PreserveSig] int GetStringValue(ref Guid key, [MarshalAs(UnmanagedType.LPWStr)] out string value);
        [PreserveSig] int GetValue(ref Guid key, IntPtr value);
    }

    [ComImport, Guid("FD672C54-40EA-4D6E-9B49-CFB1A7507BD7"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    public interface ICredentialProviderCredential2 : ICredentialProviderCredential
    {
        [PreserveSig] int GetUserSid([MarshalAs(UnmanagedType.LPWStr)] out string sid);
    }

    public enum CREDENTIAL_PROVIDER_USAGE_SCENARIO { CPUS_LOGON = 1, CPUS_UNLOCK_WORKSTATION = 2 }
    public enum CREDENTIAL_PROVIDER_FIELD_STATE { CPFS_HIDDEN = 0, CPFS_DISPLAY_IN_SELECTED_TILE = 1, CPFS_DISPLAY_IN_DESELECTED_TILE = 2, CPFS_DISPLAY_IN_BOTH = 3 }
    public enum CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE { CPFIS_NONE = 0, CPFIS_READONLY = 1, CPFIS_DISABLED = 2, CPFIS_FOCUSED = 3 }
    public enum CREDENTIAL_PROVIDER_FIELD_TYPE { CPFT_INVALID = 0, CPFT_LARGE_TEXT = 1, CPFT_SMALL_TEXT = 2, CPFT_COMMAND_LINK = 3, CPFT_EDIT_TEXT = 4, CPFT_PASSWORD_TEXT = 5, CPFT_TILE_IMAGE = 6, CPFT_CHECKBOX = 7, CPFT_COMBOBOX = 8, CPFT_SUBMIT_BUTTON = 9 }
    public enum CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE { CPGSR_NO_CREDENTIAL_NOT_FINISHED = 0, CPGSR_NO_CREDENTIAL_FINISHED = 1, CPGSR_RETURN_CREDENTIAL_FINISHED = 2, CPGSR_RETURN_NO_CREDENTIAL_FINISHED = 3 }
    public enum CREDENTIAL_PROVIDER_STATUS_ICON { CPSI_NONE = 0, CPSI_ERROR = 1, CPSI_WARNING = 2, CPSI_SUCCESS = 3 }

    [StructLayout(LayoutKind.Sequential)]
    public struct CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION
    {
        public uint ulAuthenticationPackage;
        public Guid clsidCredentialProvider;
        public uint cbSerialization;
        public IntPtr rgbSerialization;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR
    {
        public uint dwFieldID;
        public CREDENTIAL_PROVIDER_FIELD_TYPE cpft;
        public IntPtr pszLabel;
        public Guid guidFieldType;
    }

    public static class HRESULT
    {
        public const int S_OK = 0;
        public const int E_NOTIMPL = unchecked((int)0x80004001);
        public const int E_FAIL = unchecked((int)0x80004005);
    }

    public static class NativeMethods
    {
        public const int STATUS_SUCCESS = 0;
        public const uint KerbWorkstationUnlockLogon = 7;

        [DllImport("secur32.dll", CharSet = CharSet.Auto)]
        public static extern int LsaConnectUntrusted(out IntPtr LsaHandle);

        [DllImport("secur32.dll", CharSet = CharSet.Auto)]
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
    public class WristKeyCredentialProvider : ICredentialProvider, ICredentialProviderSetUserArray
    {
        private ICredentialProviderEvents _events;
        private WristKeyCredential _credential;

        public int SetUsageScenario(CREDENTIAL_PROVIDER_USAGE_SCENARIO cpus, uint dwFlags)
        {
            if (cpus == CREDENTIAL_PROVIDER_USAGE_SCENARIO.CPUS_LOGON ||
                cpus == CREDENTIAL_PROVIDER_USAGE_SCENARIO.CPUS_UNLOCK_WORKSTATION)
            {
                _credential = null;
                return HRESULT.S_OK;
            }
            return HRESULT.E_NOTIMPL;
        }

        public int SetSerialization(ref CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION pcpcs)
        {
            return HRESULT.S_OK;
        }

        public int Advise(ICredentialProviderEvents pcpe, ulong upAdviseContext)
        {
            _events = pcpe;
            return HRESULT.S_OK;
        }

        public int UnAdvise()
        {
            _events = null;
            return HRESULT.S_OK;
        }

        public int GetFieldDescriptorCount(out uint pdwCount)
        {
            pdwCount = WristKeyCredential.FieldCount;
            return HRESULT.S_OK;
        }

        public int GetFieldDescriptorAt(uint dwIndex, out IntPtr ppcpfd)
        {
            ppcpfd = IntPtr.Zero;
            if (dwIndex >= WristKeyCredential.FieldCount)
                return unchecked((int)0x80070057);

            var descriptor = WristKeyCredential.GetFieldDescriptor(dwIndex);
            ppcpfd = Marshal.AllocCoTaskMem(Marshal.SizeOf(typeof(CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR)));
            Marshal.StructureToPtr(descriptor, ppcpfd, false);
            return HRESULT.S_OK;
        }

        public int GetCredentialCount(out uint pdwCount, out uint pdwDefault, out int pbAutoLogonWithDefault)
        {
            pdwCount = _credential == null ? 0u : 1u;
            pdwDefault = unchecked((uint)-1);
            pbAutoLogonWithDefault = 0;
            return HRESULT.S_OK;
        }

        public int GetCredentialAt(uint dwIndex, out ICredentialProviderCredential ppcpc)
        {
            if (dwIndex == 0 && _credential != null)
            {
                ppcpc = _credential;
                return HRESULT.S_OK;
            }
            ppcpc = null;
            return unchecked((int)0x80070057);
        }

        public int SetUserArray(ICredentialProviderUserArray users)
        {
            _credential = null;
            if (users == null)
                return HRESULT.S_OK;

            uint count;
            if (users.GetCount(out count) != HRESULT.S_OK || count == 0)
                return HRESULT.S_OK;

            ICredentialProviderUser user;
            if (users.GetAt(0, out user) != HRESULT.S_OK || user == null)
                return HRESULT.S_OK;

            string sid;
            if (user.GetSid(out sid) == HRESULT.S_OK && !string.IsNullOrEmpty(sid))
                _credential = new WristKeyCredential(sid);
            return HRESULT.S_OK;
        }
    }

    [ComVisible(true)]
    [ClassInterface(ClassInterfaceType.None)]
    public class WristKeyCredential : ICredentialProviderCredential, ICredentialProviderCredential2
    {
        public const uint FieldCount = 3;
        private static readonly Guid FieldTypeGuid = Guid.Empty;
        private ICredentialProviderCredentialEvents _events;
        private readonly string _userSid;

        public WristKeyCredential(string userSid)
        {
            _userSid = userSid;
        }

        public int GetUserSid(out string sid)
        {
            sid = _userSid;
            return HRESULT.S_OK;
        }

        public static CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR GetFieldDescriptor(uint fieldId)
        {
            string label;
            CREDENTIAL_PROVIDER_FIELD_TYPE type;
            switch (fieldId)
            {
                case 0:
                    label = "WristKey Unlock";
                    type = CREDENTIAL_PROVIDER_FIELD_TYPE.CPFT_LARGE_TEXT;
                    break;
                case 1:
                    label = "Status";
                    type = CREDENTIAL_PROVIDER_FIELD_TYPE.CPFT_SMALL_TEXT;
                    break;
                default:
                    label = "Unlock";
                    type = CREDENTIAL_PROVIDER_FIELD_TYPE.CPFT_SUBMIT_BUTTON;
                    break;
            }

            return new CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR
            {
                dwFieldID = fieldId,
                cpft = type,
                pszLabel = Marshal.StringToCoTaskMemUni(label),
                guidFieldType = FieldTypeGuid
            };
        }

        public int Advise(ICredentialProviderCredentialEvents pcpce)
        {
            _events = pcpce;
            return HRESULT.S_OK;
        }

        public int UnAdvise()
        {
            _events = null;
            return HRESULT.S_OK;
        }

        public int SetSelected(out int pbAutoLogon)
        {
            pbAutoLogon = 0;
            return HRESULT.S_OK;
        }

        public int SetDeselected()
        {
            return HRESULT.S_OK;
        }

        public int GetFieldState(uint dwFieldId, out CREDENTIAL_PROVIDER_FIELD_STATE pcpfs, out CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE pcpfis)
        {
            pcpfs = dwFieldId == 2
                ? CREDENTIAL_PROVIDER_FIELD_STATE.CPFS_DISPLAY_IN_SELECTED_TILE
                : CREDENTIAL_PROVIDER_FIELD_STATE.CPFS_DISPLAY_IN_BOTH;
            pcpfis = dwFieldId == 2
                ? CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE.CPFIS_FOCUSED
                : CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE.CPFIS_NONE;
            return HRESULT.S_OK;
        }

        public int GetStringValue(uint dwFieldId, out string ppsz)
        {
            if (dwFieldId == 0)
                ppsz = "WristKey Unlock";
            else if (dwFieldId == 1)
                ppsz = "Bring your watch close to unlock...";
            else
                ppsz = "";
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
            return dwFieldId == 2 ? HRESULT.S_OK : HRESULT.E_NOTIMPL;
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

        public int SetStringValue(uint dwFieldId, string psz)
        {
            return HRESULT.S_OK;
        }

        public int SetCheckboxValue(uint dwFieldId, int bChecked)
        {
            return HRESULT.E_NOTIMPL;
        }

        public int SetComboBoxSelectedValue(uint dwFieldId, uint dwSelectedItem)
        {
            return HRESULT.E_NOTIMPL;
        }

        public int CommandLinkClicked(uint dwFieldId)
        {
            return HRESULT.E_NOTIMPL;
        }

        public int GetSerialization(out CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE pcpgsr,
            out CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION pcpcs, out string ppszOptionalStatusText,
            out int pcpsiOptionalStatusIcon)
        {
            ppszOptionalStatusText = null;
            pcpsiOptionalStatusIcon = 0;
            pcpcs = new CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION();

            try
            {
                string password = GetPasswordFromDaemon();
                if (string.IsNullOrEmpty(password))
                {
                    pcpgsr = CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE.CPGSR_NO_CREDENTIAL_NOT_FINISHED;
                    return HRESULT.S_OK;
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
                return HRESULT.S_OK;
            }
            catch (Exception ex)
            {
                ppszOptionalStatusText = "WristKey error: " + ex.Message;
                pcpsiOptionalStatusIcon = (int)CREDENTIAL_PROVIDER_STATUS_ICON.CPSI_ERROR;
                pcpgsr = CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE.CPGSR_NO_CREDENTIAL_NOT_FINISHED;
                return HRESULT.E_FAIL;
            }
        }

        public int ReportResult(int ntsStatus, int ntsSubstatus, out string ppszOptionalStatusText, out int pcpsiOptionalStatusIcon)
        {
            ppszOptionalStatusText = null;
            pcpsiOptionalStatusIcon = 0;
            return HRESULT.S_OK;
        }

        private string GetPasswordFromDaemon()
        {
            try
            {
                using (NamedPipeClientStream client = new NamedPipeClientStream(".", "WristKeyUnlock",
                    PipeDirection.InOut, PipeOptions.None, TokenImpersonationLevel.Impersonation))
                {
                    client.Connect(5000);
                    using (StreamWriter writer = new StreamWriter(client, Encoding.UTF8, 1024, true) { AutoFlush = true })
                    {
                        // WriteLine добавляет \r\n — daemon читает через from_slice и падает.
                        // Используем Write + \n без \r.
                        string json = "{\"action\":\"unlock\",\"user\":\"" +
                            Environment.UserName.Replace("\\", "\\\\").Replace("\"", "\\\"") + "\"}";
                        writer.Write(json + "\n");
                        writer.Flush();
                    }
                    using (StreamReader reader = new StreamReader(client, Encoding.UTF8, false, 1024, true))
                    {
                        string responseJson = reader.ReadToEnd().TrimEnd('\r', '\n', '\0');
                        if (string.IsNullOrEmpty(responseJson))
                            throw new Exception("Empty response from daemon");
                        var response = DeserializeJson<UnlockResponse>(responseJson);
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
                // Fallback: try old pipe read for backward compatibility
                try
                {
                    using (NamedPipeClientStream client = new NamedPipeClientStream(".", "WristKeyUnlock",
                        PipeDirection.In, PipeOptions.None, TokenImpersonationLevel.Impersonation))
                    {
                        client.Connect(2000);
                        using (StreamReader reader = new StreamReader(client, Encoding.UTF8))
                        {
                            return reader.ReadLine();
                        }
                    }
                }
                catch { }
                throw new Exception("Failed to get password from daemon: " + ex.Message);
            }
        }

        private uint GetAuthenticationPackage()
        {
            IntPtr lsaHandle;
            if (NativeMethods.LsaConnectUntrusted(out lsaHandle) != NativeMethods.STATUS_SUCCESS)
                return 2;

            NativeMethods.LSA_STRING packageName = new NativeMethods.LSA_STRING
            {
                Length = (ushort)"Kerberos".Length,
                MaximumLength = (ushort)("Kerberos".Length + 1),
                Buffer = Marshal.StringToHGlobalAnsi("Kerberos")
            };

            uint authPackage;
            int result = NativeMethods.LsaLookupAuthenticationPackage(lsaHandle, ref packageName, out authPackage);

            Marshal.FreeHGlobal(packageName.Buffer);
            NativeMethods.LsaDeregisterLogonProcess(lsaHandle);

            return result == NativeMethods.STATUS_SUCCESS ? authPackage : 2;
        }

        private static T DeserializeJson<T>(string json)
        {
            var serializer = new DataContractJsonSerializer(typeof(T));
            using (var stream = new MemoryStream(Encoding.UTF8.GetBytes(json)))
            {
                return (T)serializer.ReadObject(stream);
            }
        }

        private byte[] SerializeKerbInteractiveUnlockLogon(string username, string password, string domain)
        {
            int headerSize = Marshal.SizeOf(typeof(NativeMethods.KERB_INTERACTIVE_UNLOCK_LOGON));
            byte[] userBytes = Encoding.Unicode.GetBytes(username + "\0");
            byte[] domainBytes = Encoding.Unicode.GetBytes(domain + "\0");
            byte[] passwordBytes = Encoding.Unicode.GetBytes(password + "\0");
            int userOffset = headerSize;
            int domainOffset = userOffset + userBytes.Length;
            int passwordOffset = domainOffset + domainBytes.Length;

            NativeMethods.KERB_INTERACTIVE_UNLOCK_LOGON logon = new NativeMethods.KERB_INTERACTIVE_UNLOCK_LOGON
            {
                Logon = new NativeMethods.KERB_INTERACTIVE_LOGON
                {
                    MessageType = NativeMethods.KerbWorkstationUnlockLogon,
                }
            };

            logon.Logon.UserName = new NativeMethods.UNICODE_STRING
            {
                Length = (ushort)(username.Length * 2),
                MaximumLength = (ushort)((username.Length + 1) * 2),
                Buffer = (IntPtr)userOffset
            };

            logon.Logon.Domain = new NativeMethods.UNICODE_STRING
            {
                Length = (ushort)(domain.Length * 2),
                MaximumLength = (ushort)((domain.Length + 1) * 2),
                Buffer = (IntPtr)domainOffset
            };

            logon.Logon.Password = new NativeMethods.UNICODE_STRING
            {
                Length = (ushort)(password.Length * 2),
                MaximumLength = (ushort)((password.Length + 1) * 2),
                Buffer = (IntPtr)passwordOffset
            };

            IntPtr pLogon = Marshal.AllocCoTaskMem(headerSize);
            try
            {
                Marshal.StructureToPtr(logon, pLogon, false);
                byte[] result = new byte[passwordOffset + passwordBytes.Length];
                Marshal.Copy(pLogon, result, 0, headerSize);
                Buffer.BlockCopy(userBytes, 0, result, userOffset, userBytes.Length);
                Buffer.BlockCopy(domainBytes, 0, result, domainOffset, domainBytes.Length);
                Buffer.BlockCopy(passwordBytes, 0, result, passwordOffset, passwordBytes.Length);
                return result;
            }
            finally
            {
                Marshal.FreeCoTaskMem(pLogon);
            }
        }
    }
}
