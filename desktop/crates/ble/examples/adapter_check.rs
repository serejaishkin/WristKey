use btleplug::api::{Central, Manager as _, ScanFilter};
use btleplug::platform::Manager;
use std::time::Duration;

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
        let info = adapter
            .adapter_info()
            .await
            .unwrap_or_else(|e| format!("<error: {e}>"));
        let state = adapter.adapter_state().await;

        println!("adapter[{index}]: info={info}");
        println!("adapter[{index}]: state={state:?}");

        println!("adapter[{index}]: starting BLE scan for 15 seconds...");
        adapter.start_scan(ScanFilter::default()).await?;

        for second in 1..=15 {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let peripherals = adapter.peripherals().await?;

            println!("scan[{second}s]: peripherals={}", peripherals.len());

            for peripheral in peripherals {
                let properties = match peripheral.properties().await {
                    Ok(Some(properties)) => properties,
                    Ok(None) => continue,
                    Err(error) => {
                        println!("  properties_error={error}");
                        continue;
                    }
                };

                println!(
                    "  device: name={:?}, address={}, rssi={:?}, tx_power={:?}",
                    properties.local_name,
                    properties.address,
                    properties.rssi,
                    properties.tx_power_level
                );
            }
        }

        adapter.stop_scan().await?;
        println!("adapter[{index}]: scan stopped");
    }

    Ok(())
}
