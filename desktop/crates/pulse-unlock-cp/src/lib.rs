use windows::{
    core::{BSTR, GUID, HSTRING, PWSTR, Result},
    Win32::Foundation::{BOOL, HWND, PSID},
    Win32::Security::Authentication::Identity::{
        KERB_INTERACTIVE_LOGON, KERB_INTERACTIVE_UNLOCK_LOGON, KERB_WORKSTATION_UNLOCK_LOGON,
    },
    Win32::Security::Credentials::{
        CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION, CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR,
        CREDENTIAL_PROVIDER_FIELD_STATE, CREDENTIAL_PROVIDER_FIELD_TYPE,
        CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE, ICredentialProvider,
        ICredentialProviderCredential, ICredentialProviderCredentialEvents,
        ICredentialProviderEvents, CPFS_DISPLAY_IN_BOTH, CPFT_LARGE_TEXT, CPFT_SUBMIT_BUTTON,
        CPGSR_RETURN_CREDENTIAL_FINISHED,
    },
    Win32::System::Com::{CoTaskMemAlloc, CoTaskMemFree},
};
use std::ffi::c_void;

// GUID из твоего оригинального проекта
const CLSID_WRISTKEY: GUID = GUID::from_u128(0xA1B2C3D4_E5F6_7890_ABCD_EF1234567895);

pub struct WristKeyProvider {
    events: Option<ICredentialProviderEvents>,
    advise_context: u64,
}

impl Default for WristKeyProvider {
    fn default() -> Self {
        Self {
            events: None,
            advise_context: 0,
        }
    }
}

// --- ICredentialProvider Implementation ---
#[allow(non_snake_case)]
impl ICredentialProvider for WristKeyProvider {
    fn SetUsageScenario(&self, cpus: i32, _dwflags: u32) -> Result<()> {
        // 1 = Logon, 2 = Unlock Workstation
        if cpus == 1 || cpus == 2 {
            Ok(())
        } else {
            Err(windows::core::Error::from(windows::Win32::Foundation::E_NOTIMPL))
        }
    }

    fn SetSerialization(&self, _pcpcs: *const CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION) -> Result<()> {
        Ok(())
    }

    fn Advise(&self, pcpe: &Option<ICredentialProviderEvents>, upadvisecontext: u64) -> Result<()> {
        self.events.clone_from(pcpe);
        self.advise_context = upadvisecontext;
        Ok(())
    }

    fn UnAdvise(&self) -> Result<()> {
        self.events = None;
        Ok(())
    }

    fn GetFieldDescriptorCount(&self) -> Result<u32> {
        Ok(2) // Текстовое поле + Кнопка
    }

    fn GetFieldDescriptorAt(&self, dwindex: u32) -> Result<CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR> {
        let desc = match dwindex {
            0 => CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR {
                dwFieldID: 0,
                cpft: CPFT_LARGE_TEXT,
                pszLabel: PWSTR::from_raw(HSTRING::from("WristKey: Поднесите часы для разблокировки").as_ptr() as *mut _),
            },
            1 => CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR {
                dwFieldID: 1,
                cpft: CPFT_SUBMIT_BUTTON,
                pszLabel: PWSTR::from_raw(HSTRING::from("Разблокировать").as_ptr() as *mut _),
            },
            _ => return Err(windows::core::Error::from(windows::Win32::Foundation::E_INVALIDARG)),
        };
        Ok(desc)
    }

    fn GetCredentialCount(&self) -> Result<(u32, u32, BOOL)> {
        Ok((1, 0, false.into())) // 1 креденшел, не по умолчанию, без автологона
    }

    fn GetCredentialAt(&self, _dwindex: u32) -> Result<ICredentialProviderCredential> {
        // Возвращаем себя же, так как мы реализуем оба интерфейса
        Ok(self.clone().into())
    }
}

// --- ICredentialProviderCredential Implementation ---
#[allow(non_snake_case)]
impl ICredentialProviderCredential for WristKeyProvider {
    fn Advise(&self, _pcpce: &Option<ICredentialProviderCredentialEvents>) -> Result<()> { Ok(()) }
    fn UnAdvise(&self) -> Result<()> { Ok(()) }
    fn SetSelected(&self, _pbautologon: *mut BOOL) -> Result<()> { Ok(()) }
    fn SetDeselected(&self) -> Result<()> { Ok(()) }
    
    fn GetFieldState(&self, _dwfieldid: u32) -> Result<(CREDENTIAL_PROVIDER_FIELD_STATE, i32)> {
        Ok((CPFS_DISPLAY_IN_BOTH, 0))
    }

    fn GetStringValue(&self, dwfieldid: u32) -> Result<PWSTR> {
        let s = match dwfieldid {
            0 => "WristKey Authentication",
            1 => "Unlock",
            _ => "",
        };
        Ok(PWSTR::from_raw(HSTRING::from(s).as_ptr() as *mut _))
    }

    // ... (остальные геттеры возвращают E_NOTIMPL, так как у нас только текст и кнопка)
    fn GetBitmapValue(&self, _dwfieldid: u32) -> Result<*mut c_void> { Err(windows::core::Error::from(windows::Win32::Foundation::E_NOTIMPL)) }
    fn GetCheckboxValue(&self, _dwfieldid: u32) -> Result<(BOOL, PWSTR)> { Err(windows::core::Error::from(windows::Win32::Foundation::E_NOTIMPL)) }
    fn GetSubmitButtonValue(&self, _dwfieldid: u32) -> Result<u32> { Err(windows::core::Error::from(windows::Win32::Foundation::E_NOTIMPL)) }
    fn GetComboBoxValueCount(&self, _dwfieldid: u32) -> Result<(u32, u32)> { Err(windows::core::Error::from(windows::Win32::Foundation::E_NOTIMPL)) }
    fn GetComboBoxValueAt(&self, _dwfieldid: u32, _dwitem: u32) -> Result<PWSTR> { Err(windows::core::Error::from(windows::Win32::Foundation::E_NOTIMPL)) }
    fn SetStringValue(&self, _dwfieldid: u32, _psz: &windows::core::PCWSTR) -> Result<()> { Ok(()) }

    fn GetSerialization(&self) -> Result<(CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE, CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION, PWSTR, i32)> {
        // 1. Запрашиваем пароль у демона через Named Pipe
        let password = match request_password_from_daemon() {
            Ok(p) => p,
            Err(e) => {
                let err_msg = HSTRING::from(format!("WristKey Error: {}", e));
                return Ok((
                    CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE::default(),
                    unsafe { std::mem::zeroed() },
                    PWSTR::from_raw(err_msg.as_ptr() as *mut _),
                    3, // CPSI_ERROR
                ));
            }
        };

        // 2. Формируем непрерывный блок памяти для LSA (ИСПРАВЛЕНИЕ C# БАГА)
        let username = std::env::var("USERNAME").unwrap_or_else(|_| "User".to_string());
        let domain = std::env::var("USERDOMAIN").unwrap_or_else(|_| ".".to_string());

        let serialized_data = build_kerb_unlock_blob(&username, &domain, &password);

        // 3. Выделяем память через CoTaskMemAlloc (Windows сама освободит её)
        let ptr = unsafe { CoTaskMemAlloc(serialized_data.len()) };
        if ptr.is_null() {
            return Err(windows::core::Error::from(windows::Win32::Foundation::E_OUTOFMEMORY));
        }
        unsafe {
            std::ptr::copy_nonoverlapping(serialized_data.as_ptr(), ptr as *mut u8, serialized_data.len());
        }

        let mut pcpcs: CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION = unsafe { std::mem::zeroed() };
        pcpcs.ulAuthenticationPackage = 2; // Negoziate/Kerberos fallback
        pcpcs.clsidCredentialProvider = CLSID_WRISTKEY;
        pcpcs.cbSerialization = serialized_data.len() as u32;
        pcpcs.rgbSerialization = ptr;

        Ok((
            CPGSR_RETURN_CREDENTIAL_FINISHED,
            pcpcs,
            PWSTR::null(),
            0, // CPSI_NONE
        ))
    }

    fn ReportResult(&self, _ntsstatus: i32, _ntssubstatus: i32) -> Result<(PWSTR, i32)> {
        Ok((PWSTR::null(), 0))
    }
}

// --- Вспомогательные функции ---

fn request_password_from_daemon() -> std::result::Result<String, String> {
    // Упрощенный синхронный запрос к pipe. 
    // В продакшене лучше использовать неблокирующий вызов с таймаутом.
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt;
    use std::io::{Read, Write};

    // Открываем pipe. Демон ДОЛЖЕН быть запущен с правами доступа для SYSTEM/Everyone
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(0x00000080) // FILE_FLAG_OVERLAPPED (упрощенно, лучше использовать нормальный async или таймаут)
        .open(r"\\.\pipe\WristKeyUnlock")
        .map_err(|e| format!("Pipe open failed: {}", e))?;

    let request = serde_json::json!({
        "action": "unlock",
        "user": std::env::var("USERNAME").unwrap_or_default()
    });
    
    let mut req_bytes = request.to_string().into_bytes();
    req_bytes.push(b'\n');
    
    file.write_all(&req_bytes).map_err(|e| format!("Write failed: {}", e))?;
    
    let mut response = String::new();
    file.read_to_string(&mut response).map_err(|e| format!("Read failed: {}", e))?;

    let parsed: serde_json::Value = serde_json::from_str(&response).map_err(|e| format!("JSON parse: {}", e))?;
    
    if parsed["status"] == "success" {
        Ok(parsed["password"].as_str().unwrap_or("").to_string())
    } else {
        Err(parsed["message"].as_str().unwrap_or("Unknown error").to_string())
    }
}

fn build_kerb_unlock_blob(username: &str, domain: &str, password: &str) -> Vec<u8> {
    // Преобразуем строки в UTF-16 с нулевым терминатором
    let mut user_w: Vec<u16> = username.encode_utf16().chain(std::iter::once(0)).collect();
    let mut domain_w: Vec<u16> = domain.encode_utf16().chain(std::iter::once(0)).collect();
    let mut pass_w: Vec<u16> = password.encode_utf16().chain(std::iter::once(0)).collect();

    let struct_size = std::mem::size_of::<KERB_INTERACTIVE_UNLOCK_LOGON>();
    
    // Вычисляем смещения для строк внутри одного непрерывного буфера
    let user_offset = struct_size;
    let domain_offset = user_offset + user_w.len() * 2;
    let pass_offset = domain_offset + domain_w.len() * 2;
    let total_size = pass_offset + pass_w.len() * 2;

    let mut buffer = vec![0u8; total_size];

    // Заполняем структуру
    let mut logon = KERB_INTERACTIVE_UNLOCK_LOGON {
        Logon: KERB_INTERACTIVE_LOGON {
            MessageType: KERB_WORKSTATION_UNLOCK_LOGON,
            UserName: windows::Win32::Foundation::UNICODE_STRING {
                Length: ((user_w.len() - 1) * 2) as u16,
                MaximumLength: (user_w.len() * 2) as u16,
                Buffer: PWSTR((buffer.as_mut_ptr() as usize + user_offset) as *mut _),
            },
            Domain: windows::Win32::Foundation::UNICODE_STRING {
                Length: ((domain_w.len() - 1) * 2) as u16,
                MaximumLength: (domain_w.len() * 2) as u16,
                Buffer: PWSTR((buffer.as_mut_ptr() as usize + domain_offset) as *mut _),
            },
            Password: windows::Win32::Foundation::UNICODE_STRING {
                Length: ((pass_w.len() - 1) * 2) as u16,
                MaximumLength: (pass_w.len() * 2) as u16,
                Buffer: PWSTR((buffer.as_mut_ptr() as usize + pass_offset) as *mut _),
            },
        },
        LogonId: unsafe { std::mem::zeroed() },
    };

    unsafe {
        // Копируем заголовок структуры в начало буфера
        std::ptr::copy_nonoverlapping(&logon as *const _ as *const u8, buffer.as_mut_ptr(), struct_size);
        
        // Копируем строки в их смещения
        std::ptr::copy_nonoverlapping(user_w.as_ptr() as *const u8, buffer.as_mut_ptr().add(user_offset), user_w.len() * 2);
        std::ptr::copy_nonoverlapping(domain_w.as_ptr() as *const u8, buffer.as_mut_ptr().add(domain_offset), domain_w.len() * 2);
        std::ptr::copy_nonoverlapping(pass_w.as_ptr() as *const u8, buffer.as_mut_ptr().add(pass_offset), pass_w.len() * 2);
    }

    buffer
}

// Точка входа для регистрации COM (опционально, но полезно)
#[no_mangle]
pub extern "system" fn DllRegisterServer() -> i32 {
    // Здесь можно добавить код для автоматической регистрации в реестре,
    // но проще использовать PowerShell скрипт ниже.
    0 // S_OK
}