using System;
using Microsoft.Win32;

namespace WristKeyCredentialProvider
{
    /// <summary>
    /// Registration helper for the WristKey Credential Provider.
    /// Run as Administrator: WristKeyCredentialProvider.exe --register
    /// </summary>
    class Program
    {
        private static readonly string Clsid = "{A1B2C3D4-E5F6-7890-ABCD-EF1234567895}";
        private static readonly string ProviderName = "WristKeyCredentialProvider";

        static void Main(string[] args)
        {
            if (args.Length > 0 && args[0] == "--register")
            {
                Register();
                Console.WriteLine("WristKey Credential Provider registered successfully.");
                Console.WriteLine("Restart your computer to see the WristKey tile.");
            }
            else if (args.Length > 0 && args[0] == "--unregister")
            {
                Unregister();
                Console.WriteLine("WristKey Credential Provider unregistered.");
            }
            else
            {
                Console.WriteLine("Usage: WristKeyCredentialProvider.exe [--register|--unregister]");
            }
        }

        static void Register()
        {
            string dllPath = System.Reflection.Assembly.GetExecutingAssembly().Location;

            // Register COM class
            using (var clsidKey = Registry.ClassesRoot.CreateSubKey(@"CLSID\\" + Clsid))
            {
                clsidKey.SetValue(null, "WristKey Credential Provider");
                using (var inprocKey = clsidKey.CreateSubKey("InprocServer32"))
                {
                    inprocKey.SetValue(null, dllPath);
                    inprocKey.SetValue("ThreadingModel", "Apartment");
                }
            }

            // Register as Credential Provider
            using (var cpKey = Registry.LocalMachine.CreateSubKey(
                @"SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Authentication\\Credential Providers\\" + Clsid))
            {
                cpKey.SetValue(null, ProviderName);
            }
        }

        static void Unregister()
        {
            Registry.ClassesRoot.DeleteSubKeyTree(@"CLSID\\" + Clsid, false);
            Registry.LocalMachine.DeleteSubKeyTree(
                @"SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Authentication\\Credential Providers\\" + Clsid, false);
        }
    }
}
