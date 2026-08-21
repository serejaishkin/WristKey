use btleplug::api::{Central, Manager as _};
use btleplug::platform::Manager;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("WristKey BLE adapter diagnostic");

    let manager = Manager::new().await?;
    let adapters = manager.adapters().await?;

    println!("adapter_count={}", adapters.len());

    if adapters.is_empty() {
        println!("NO_BLE_ADAPTER");
        println!("Check Windows Bluetooth radio, driver and bthserv service.");
        return Ok(());
    }

    for (index, adapter) in adapters.iter().enumerate() {
        let info = adapter.adapter_info().await.unwrap_or_else(|e| format!("<error: {e}>"));
        let state = adapter.adapter_state().await;
        let peripherals = adapter.peripherals().await;

        println!("adapter[{index}]: info={info}");
        println!("adapter[{index}]: state={state:?}");
        match peripherals {
            Ok(items) => println!("adapter[{index}]: cached_peripherals={}", items.len()),
            Err(e) => println!("adapter[{index}]: peripherals_error={e}"),
        }
    }

    Ok(())
}
