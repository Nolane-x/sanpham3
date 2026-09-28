#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProbeKind {
    InterfaceInventory,
    Ipv4,
    Ipv6,
    Dns,
    Udp,
    Tcp,
    TinyHttps,
    LanPeer,
    Bluetooth,
    WifiDirect,
    WifiAware,
    Cellular,
    Satellite,
    Tunnel,
    Other,
}

impl ProbeKind {
    pub fn proves_information_path(self) -> bool {
        matches!(
            self,
            Self::Dns
                | Self::Udp
                | Self::Tcp
                | Self::TinyHttps
                | Self::LanPeer
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeStatus {
    Pending,
    Succeeded,
    Failed,
    Blocked,
    Unsupported,
}

impl ProbeStatus {
    pub fn terminal(self) -> bool {
        self != Self::Pending
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeRecord {
    pub id: String,
    pub kind: ProbeKind,
    pub status: ProbeStatus,
    pub detail: Option<String>,
}

#[derive(Debug, Default)]
pub struct RecoveryLedger {
    records: Vec<ProbeRecord>,
}

impl RecoveryLedger {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        id: impl Into<String>,
        kind: ProbeKind,
    ) -> bool {
        let id = id.into();
        if self.records.iter().any(|record| record.id == id) {
            return false;
        }

        self.records.push(ProbeRecord {
            id,
            kind,
            status: ProbeStatus::Pending,
            detail: None,
        });
        true
    }

    pub fn set_status(
        &mut self,
        id: &str,
        status: ProbeStatus,
        detail: Option<String>,
    ) -> bool {
        let Some(record) =
            self.records.iter_mut().find(|record| record.id == id)
        else {
            return false;
        };

        record.status = status;
        record.detail = detail;
        true
    }

    pub fn records(&self) -> &[ProbeRecord] {
        &self.records
    }

    pub fn exhaustive_complete(&self) -> bool {
        !self.records.is_empty()
            && self
                .records
                .iter()
                .all(|record| record.status.terminal())
    }

    pub fn any_information_path(&self) -> bool {
        self.records.iter().any(|record| {
            record.status == ProbeStatus::Succeeded
                && record.kind.proves_information_path()
        })
    }

    /// LOCAL_ONLY is a defensible conclusion only when every registered probe
    /// reached a terminal state and none found a usable information path.
    pub fn can_declare_local_only(&self) -> bool {
        self.exhaustive_complete() && !self.any_information_path()
    }

    pub fn pending_count(&self) -> usize {
        self.records
            .iter()
            .filter(|record| record.status == ProbeStatus::Pending)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_probe_blocks_local_only() {
        let mut ledger = RecoveryLedger::new();
        ledger.register("wifi:ipv4", ProbeKind::Ipv4);
        ledger.register("wifi:peer", ProbeKind::LanPeer);

        ledger.set_status(
            "wifi:ipv4",
            ProbeStatus::Failed,
            Some("no route".to_owned()),
        );

        assert_eq!(ledger.pending_count(), 1);
        assert!(!ledger.can_declare_local_only());
    }

    #[test]
    fn terminal_failures_allow_local_only() {
        let mut ledger = RecoveryLedger::new();
        ledger.register("wifi:ipv4", ProbeKind::Ipv4);
        ledger.register("wifi:peer", ProbeKind::LanPeer);

        ledger.set_status("wifi:ipv4", ProbeStatus::Failed, None);
        ledger.set_status("wifi:peer", ProbeStatus::Unsupported, None);

        assert!(ledger.exhaustive_complete());
        assert!(ledger.can_declare_local_only());
    }


    #[test]
    fn route_presence_alone_does_not_prevent_local_only() {
        let mut ledger = RecoveryLedger::new();
        ledger.register("wifi:ipv4", ProbeKind::Ipv4);
        ledger.register("wifi:dns", ProbeKind::Dns);

        ledger.set_status("wifi:ipv4", ProbeStatus::Succeeded, None);
        ledger.set_status("wifi:dns", ProbeStatus::Failed, None);

        assert!(ledger.exhaustive_complete());
        assert!(ledger.can_declare_local_only());
    }

    #[test]
    fn information_path_success_prevents_local_only() {
        let mut ledger = RecoveryLedger::new();
        ledger.register("wifi:udp", ProbeKind::Udp);
        ledger.register("wifi:tcp", ProbeKind::Tcp);

        ledger.set_status("wifi:udp", ProbeStatus::Succeeded, None);
        ledger.set_status("wifi:tcp", ProbeStatus::Failed, None);

        assert!(ledger.exhaustive_complete());
        assert!(!ledger.can_declare_local_only());
    }
}
