use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::protocol::{MDNS_SERVICE_TYPE, PORT};

#[derive(Debug, Clone, Serialize)]
pub struct DiscoveredHost {
    pub fullname: String,
    pub host: String,
    pub port: u16,
    pub addresses: Vec<String>,
}

/// 親機: このマシンをmDNSで広告する。daemonはHostStateが保持し続ける必要がある
/// （dropすると広告が止まる）。
pub fn advertise(daemon: &ServiceDaemon, instance_name: &str) -> mdns_sd::Result<()> {
    let hostname = format!(
        "{}.local.",
        instance_name.replace(' ', "-").to_lowercase()
    );
    let service_info = ServiceInfo::new(
        MDNS_SERVICE_TYPE,
        instance_name,
        &hostname,
        "",
        PORT,
        &[] as &[(&str, &str)],
    )?
    .enable_addr_auto();
    daemon.register(service_info)
}

/// 子機: 親機をmDNSで探索し、見つかるたびにイベントをフロントへemitする。
/// 呼び出しはブロッキングするので別スレッドで実行すること。
pub fn browse_blocking(app: AppHandle, daemon: ServiceDaemon) {
    let Ok(receiver) = daemon.browse(MDNS_SERVICE_TYPE) else {
        return;
    };
    while let Ok(event) = receiver.recv() {
        match event {
            ServiceEvent::ServiceResolved(info) => {
                let host = DiscoveredHost {
                    fullname: info.fullname.clone(),
                    host: info.host.trim_end_matches('.').to_string(),
                    port: info.port,
                    addresses: info.addresses.iter().map(|a| a.to_string()).collect(),
                };
                let _ = app.emit("host-discovered", host);
            }
            ServiceEvent::ServiceRemoved(_, fullname) => {
                let _ = app.emit("host-removed", fullname);
            }
            _ => {}
        }
    }
}
