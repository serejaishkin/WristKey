using System;
using System.IO;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using Lithnet.CredentialProvider;

namespace WristKey.CredentialProvider
{
    /// <summary>
    /// WristKey Credential Provider — shows a tile on the Windows login/unlock screen.
    /// When the user clicks the tile, it contacts the WristKey daemon via named pipe
    /// to retrieve the Windows password after BLE challenge-response authentication.
    /// </summary>
    [ComVisible(true)]
    [ClassInterface(ClassInterfaceType.None)]
    [ProgId("WristKey.CredentialProvider")]
    [Guid("A1B2C3D4-E5F6-7890-ABCD-EF1234567890")]
    public class WristKeyCredentialProvider : CredentialProviderBase
    {
        public override void GetCredentialAt(uint index, out ICredentialProviderCredential credential)
        {
            credential = new WristKeyCredential(this);
        }

        public override uint GetCredentialCount(out uint count, out uint defaultIndex, out CredentialProviderFieldState fieldState)
        {
            count = 1;
            defaultIndex = 0xFFFFFFFF; // No default
            fieldState = CredentialProviderFieldState.Show;
            return 0;
        }

        public override uint GetFieldDescriptorAt(uint index, out CredentialProviderFieldDescriptor fieldDescriptor)
        {
            if (index == 0)
            {
                fieldDescriptor = new CredentialProviderFieldDescriptor
                {
                    cpft = CredentialProviderFieldType.TileImage,
                    pszLabel = "WristKey",
                    guidFieldType = Guid.Empty
                };
                return 0;
            }
            fieldDescriptor = null;
            return 1;
        }

        public override uint GetFieldDescriptorCount(out uint count)
        {
            count = 1;
            return 0;
        }

        public override uint SetUsageScenario(CredentialProviderUsageScenario cpus, CredentialProviderFlags dwFlags)
        {
            // Support both login and unlock scenarios
            if (cpus == CredentialProviderUsageScenario.Logon ||
                cpus == CredentialProviderUsageScenario.UnlockWorkstation)
            {
                return 0; // S_OK
            }
            return 0x80070032; // ERROR_NOT_SUPPORTED
        }
    }

    [ComVisible(true)]
    [ClassInterface(ClassInterfaceType.None)]
    public class WristKeyCredential : CredentialBase
    {
        private const string PipeName = @"\\.\pipe\WristKeyUnlock";
        private const int PipeTimeoutMs = 15000;

        public WristKeyCredential(ICredentialProviderEvents events) : base(events)
        {
        }

        public override uint Advise(ICredentialProviderEvents pcpe)
        {
            return 0;
        }

        public override uint UnAdvise()
        {
            return 0;
        }

        public override uint GetSerialization(
            out CredentialProviderGetSerializationResponse pcpgsr,
            out CredentialProviderCredentialSerialization pcpcs,
            out string optionalStatusText,
            out CredentialProviderStatusIcon optionalStatusIcon)
        {
            pcpgsr = CredentialProviderGetSerializationResponse.NoCredentialNotFinished;
            pcpcs = new CredentialProviderCredentialSerialization();
            optionalStatusText = null;
            optionalStatusIcon = CredentialProviderStatusIcon.None;

            try
            {
                // Contact WristKey daemon via named pipe
                string password = RequestPasswordFromDaemon();
                if (string.IsNullOrEmpty(password))
                {
                    optionalStatusText = "WristKey: No response from watch. Make sure it's nearby.";
                    optionalStatusIcon = CredentialProviderStatusIcon.Error;
                    return 0;
                }

                // Build KERB_INTERACTIVE_UNLOCK_LOGON for automatic unlock
                var serialization = new CredentialSerialization(
                    System.Security.Principal.WindowsIdentity.GetCurrent().Name,
                    password,
                    ""
                );

                pcpcs = serialization.GetCredentialSerialization();
                pcpgsr = CredentialProviderGetSerializationResponse.ReturnCredentialFinished;
                optionalStatusText = "WristKey: Unlocking...";
                optionalStatusIcon = CredentialProviderStatusIcon.Success;
                return 0;
            }
            catch (TimeoutException)
            {
                optionalStatusText = "WristKey: Watch not responding. Try again.";
                optionalStatusIcon = CredentialProviderStatusIcon.Error;
                return 0;
            }
            catch (Exception ex)
            {
                optionalStatusText = $"WristKey error: {ex.Message}";
                optionalStatusIcon = CredentialProviderStatusIcon.Error;
                return 0;
            }
        }

        public override uint ReportResult(
            int status,
            int subStatus,
            out string optionalStatusText,
            out CredentialProviderStatusIcon optionalStatusIcon)
        {
            optionalStatusText = null;
            optionalStatusIcon = CredentialProviderStatusIcon.None;
            return 0;
        }

        public override uint GetFieldState(
            uint fieldId,
            out CredentialProviderFieldState fieldState,
            out CredentialProviderFieldInteractiveState fieldInteractiveState)
        {
            fieldState = CredentialProviderFieldState.Show;
            fieldInteractiveState = CredentialProviderFieldInteractiveState.Enabled;
            return 0;
        }

        public override uint GetStringValue(uint fieldId, out string value)
        {
            value = null;
            return 0;
        }

        public override uint GetBitmapValue(uint fieldId, out IntPtr phbmp)
        {
            phbmp = IntPtr.Zero;
            return 0;
        }

        public override uint GetCheckboxValue(uint fieldId, out bool value, out string label)
        {
            value = false;
            label = null;
            return 0;
        }

        public override uint GetSubmitButtonValue(uint fieldId, out uint adjacentFieldId)
        {
            adjacentFieldId = 0;
            return 0;
        }

        public override uint GetComboBoxValueCount(uint fieldId, out uint count, out uint defaultIndex)
        {
            count = 0;
            defaultIndex = 0;
            return 0;
        }

        public override uint GetComboBoxValueAt(uint fieldId, uint index, out string value)
        {
            value = null;
            return 0;
        }

        public override uint SetStringValue(uint fieldId, string value)
        {
            return 0;
        }

        public override uint SetCheckboxValue(uint fieldId, bool value)
        {
            return 0;
        }

        public override uint SetComboBoxValue(uint fieldId, uint index)
        {
            return 0;
        }

        public override uint CommandLinkClicked(uint fieldId)
        {
            return 0;
        }

        /// <summary>
        /// Connects to the WristKey daemon via named pipe and requests the password.
        /// The daemon will perform BLE challenge-response with the watch.
        /// </summary>
        private string RequestPasswordFromDaemon()
        {
            using (var pipe = new NamedPipeClientStream(".", "WristKeyUnlock", PipeDirection.InOut))
            {
                pipe.Connect(PipeTimeoutMs);
                pipe.ReadMode = PipeTransmissionMode.Message;

                // Send unlock request with device_id (we'll use "default" for now,
                // the daemon can be enhanced to select the most recently used device)
                string request = "UNLOCK|default\n";
                byte[] requestBytes = Encoding.UTF8.GetBytes(request);
                pipe.Write(requestBytes, 0, requestBytes.Length);
                pipe.Flush();

                // Read response
                var sb = new StringBuilder();
                byte[] buffer = new byte[1024];
                do
                {
                    int bytesRead = pipe.Read(buffer, 0, buffer.Length);
                    if (bytesRead == 0) break;
                    sb.Append(Encoding.UTF8.GetString(buffer, 0, bytesRead));
                }
                while (!pipe.IsMessageComplete);

                string response = sb.ToString().Trim();
                if (response.StartsWith("OK|"))
                {
                    return response.Substring(3);
                }
                else if (response.StartsWith("FAIL|"))
                {
                    throw new Exception(response.Substring(5));
                }
                else
                {
                    throw new Exception("Invalid response from daemon");
                }
            }
        }
    }
}
