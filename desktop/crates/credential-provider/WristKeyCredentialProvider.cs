using System;
using System.IO;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Security.Principal;
using System.Text;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;

namespace WristKeyCredentialProvider
{
    [ComImport, Guid("D27C3481-5A1C-4B2E-9BDA-5D9C1D5E0F1A"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
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

    [ComImport, Guid("C27C8D5E-776A-4A85-8BFD-3F0F4C3B2A1E"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
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
        [PreserveSig] int GetSerialization(out CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE pcpgsr,
            out CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION pcpcs, out string ppszOptionalStatusText,
            out int pcpsiOptionalStatusIcon);
        [PreserveSig] int ReportResult(int ntsStatus, int ntsSubstatus, out string ppszOptionalStatusText, out int pcpsiOptionalStatusIcon);
    }

    [ComImport, Guid("F1F3D5E7-9B8A-4C2D-E6F0-1A3B5C7D9E0F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    public interface ICredentialProviderEvents
    {
        [PreserveSig] int CredentialsChanged(ulong upAdviseContext);
    }

    [ComImport, Guid("6C793AA0-1B8A-4A7B-8C3B-9B5F3E7A7E1E"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    public interface ICredentialProviderCredentialEvents
    {
        [PreserveSig] int SetFieldState(IntPtr pcpc, uint dwFieldId, CREDENTIAL_PROVIDER_FIELD_STATE cpfs);
        [PreserveSig] int SetFieldInteractiveState(IntPtr pcpc, uint dwFieldId, CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE cpfis);
        [PreserveSig] int SetFieldString(IntPtr pcpc, uint dwFieldId, string psz);
        [PreserveSig] int SetFieldCheckbox(IntPtr pcpc, uint dwFieldId, int bChecked, string pszLabel);
        [PreserveSig] int SetFieldSubmitButton(IntPtr pcpc, uint dwFieldId, uint dwAdjacentTo);
        [PreserveSig] int SetFieldBitmap(IntPtr pcpc, uint dwFieldId, IntPtr hbmp);
    }

    public enum CREDENTIAL_PROVIDER_USAGE_SCENARIO { CPUS_LOGON = 1, CPUS_UNLOCK_WORKSTATION = 2 }
    public enum CREDENTIAL_PROVIDER_FIELD_STATE { CPFS_DISPLAY_IN_SELECTED_TILE = 1 }
    public enum CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE { CPFIS_NONE = 0 }
    public enum CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE { CPGSR_NO_CREDENTIAL_NOT_FINISHED = 0, CPGSR_RETURN_CREDENTIAL_FINISHED = 1 }
    public enum CREDENTIAL_PROVIDER_STATUS_ICON { CPSI_NONE = 0, CPSI_ERROR = 3 }

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
    public class WristKeyCredentialProvider : ICredentialProvider
    {
        private ICredentialProviderEvents _events;

        public int SetUsageScenario(CREDENTIAL_PROVIDER_USAGE_SCENARIO cpus, uint dwFlags)
        {
            if (cpus == CREDENTIAL_PROVIDER_USAGE_SCENARIO.CPUS_LOGON ||
                cpus == CREDENTIAL_PROVIDER_USAGE_SCENARIO.CPUS_UNLOCK_WORKSTATION)
            {
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
            pdwCount = 2;
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
        public int Advise(ICredentialProviderCredentialEvents pcpce)
        {
            return HRESULT.S_OK;
        }

        public int UnAdvise()
        {
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
            pcpfs = CREDENTIAL_PROVIDER_FIELD_STATE.CPFS_DISPLAY_IN_SELECTED_TILE;
            pcpfis = CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE.CPFIS_NONE;
            return HRESULT.S_OK;
        }

        public int GetStringValue(uint dwFieldId, out string ppsz)
        {
            if (dwFieldId == 0)
                ppsz = "WristKey Unlock";
            else
                ppsz = "Bring your watch close to unlock...";
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

        public int SetStringValue(uint dwFieldId, string psz)
        {
            return HRESULT.S_OK;
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
                pcpcs.CredentialBlobSize = (uint)Encoding.Unicode.GetByteCount(password);
                pcpcs.CredentialBlob = Marshal.StringToCoTaskMemUni(password);

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
                    using (StreamWriter writer = new StreamWriter(client, Encoding.UTF8) { AutoFlush = true })
                    {
                        var request = new JObject();
                        request["action"] = "unlock";
                        request["user"] = Environment.UserName;
                        // WriteLine добавляет \r\n — daemon читает через from_slice и падает.
                        // Используем Write + \n без \r.
                        string json = request.ToString(Newtonsoft.Json.Formatting.None);
                        writer.Write(json + "\n");
                        writer.Flush();
                    }
                    using (StreamReader reader = new StreamReader(client, Encoding.UTF8))
                    {
                        string responseJson = reader.ReadToEnd().TrimEnd('\r', '\n', '\0');
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

        private byte[] SerializeKerbInteractiveUnlockLogon(string username, string password, string domain)
        {
            NativeMethods.KERB_INTERACTIVE_UNLOCK_LOGON logon = new NativeMethods.KERB_INTERACTIVE_UNLOCK_LOGON
            {
                Logon = new NativeMethods.KERB_INTERACTIVE_LOGON
                {
                    MessageType = NativeMethods.KerbWorkstationUnlockLogon,
                }
            };

            IntPtr pUserName = Marshal.StringToHGlobalUni(username);
            IntPtr pDomain = Marshal.StringToHGlobalUni(domain);
            IntPtr pPassword = Marshal.StringToHGlobalUni(password);

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
            return result;
        }
    }
}
