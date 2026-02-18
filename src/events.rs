use kube::runtime::events::{Event, EventType};

pub trait ResourceEvent {
    fn to_event(&self, attribute: Option<String>) -> Event;
}

pub enum HoprdEventEnum {
    Initializing,
    Running,
    Stopped,
    Failed,
    Modified,
    Deleting,
    Deleted,
}

fn unwrap_attribute(attr: &Option<String>) -> &str {
    attr.as_deref().unwrap_or("unknown")
}

impl ResourceEvent for HoprdEventEnum {
    fn to_event(&self, _: Option<String>) -> Event {
        match self {
            HoprdEventEnum::Initializing => Event {
                type_: EventType::Normal,
                reason: "Initializing".to_string(),
                note: Some("Starting the process of creating a new node".to_string()),
                action: "Initializing Hoprd node".to_owned(),
                secondary: None,
            },
            HoprdEventEnum::Running => Event {
                type_: EventType::Normal,
                reason: "Running".to_string(),
                note: Some("Hoprd node is running".to_owned()),
                action: "Node has started".to_string(),
                secondary: None,
            },
            HoprdEventEnum::Stopped => Event {
                type_: EventType::Normal,
                reason: "Stopped".to_string(),
                note: Some("Hoprd node is stopped".to_owned()),
                action: "Node has stopped".to_string(),
                secondary: None,
            },
            HoprdEventEnum::Modified => Event {
                type_: EventType::Normal,
                reason: "Modified".to_string(),
                note: Some("Hoprd node configuration change detected".to_owned()),
                action: "Node reconfigured".to_string(),
                secondary: None,
            },
            HoprdEventEnum::Deleting => Event {
                type_: EventType::Normal,
                reason: "Deleting".to_string(),
                note: Some("Hoprd node is being deleted".to_owned()),
                action: "Node deletion started".to_string(),
                secondary: None,
            },
            HoprdEventEnum::Deleted => Event {
                type_: EventType::Normal,
                reason: "Deleted".to_string(),
                note: Some("Hoprd node is deleted".to_owned()),
                action: "Node deletion finished".to_string(),
                secondary: None,
            },
            HoprdEventEnum::Failed => Event {
                type_: EventType::Warning,
                reason: "Failed".to_string(),
                note: Some("Hoprd node is in a failed status".to_owned()),
                action: "Node configuration is corrupted".to_string(),
                secondary: None,
            },
        }
    }
}

pub enum ClusterHoprdEventEnum {
    Initialized,
    NotScaled,
    Scaling,
    Failed,
    Ready,
    Deleting,
    CreatingNode,
    NodeCreated,
    DeletingNode,
    NodeDeleted,
    Modified,
}

impl ResourceEvent for ClusterHoprdEventEnum {
    fn to_event(&self, attribute: Option<String>) -> Event {
        let parsed_attribute = unwrap_attribute(&attribute);
        match self {
            ClusterHoprdEventEnum::Initialized => Event {
                type_: EventType::Normal,
                reason: "Initialized".to_string(),
                note: Some("Starting the process of creating a new cluster of hoprd".to_string()),
                action: "ClusterHoprd node initialized".to_owned(),
                secondary: None,
            },
            ClusterHoprdEventEnum::NotScaled => Event {
                type_: EventType::Warning,
                reason: "NotScaled".to_string(),
                note: Some(format!(
                    "ClusterHoprd is not scaled. There are {} nodes pending to be synchronized",
                    attribute.as_ref().unwrap_or(&"unknown".to_string())
                )),
                action: "ClusterHoprd is not scaled".to_string(),
                secondary: None,
            },
            ClusterHoprdEventEnum::Scaling => Event {
                type_: EventType::Warning,
                reason: "Scaling".to_string(),
                note: Some("ClusterHoprd is scaling to meet the required replicas".to_string()),
                action: "ClusterHoprd is scaling".to_string(),
                secondary: None,
            },
            ClusterHoprdEventEnum::Failed => Event {
                type_: EventType::Warning,
                reason: "Failed".to_string(),
                note: Some("ClusterHoprd is in failed status.".to_string()),
                action: "ClusterHoprd configuration is corrupted".to_string(),
                secondary: None,
            },
            ClusterHoprdEventEnum::Ready => Event {
                type_: EventType::Normal,
                reason: "Ready".to_string(),
                note: Some("ClusterHoprd is in ready phase".to_owned()),
                action: "ClusterHoprd is ready now".to_string(),
                secondary: None,
            },
            ClusterHoprdEventEnum::Deleting => Event {
                type_: EventType::Normal,
                reason: "Deleting".to_string(),
                note: Some("ClusterHoprd is going to be deleted".to_owned()),
                action: "ClusterHoprd is being deleted".to_string(),
                secondary: None,
            },
            ClusterHoprdEventEnum::CreatingNode => Event {
                type_: EventType::Normal,
                reason: "CreatingNode".to_string(),
                note: Some(format!("Node {} is being created in the cluster", parsed_attribute)),
                action: "A new node is being created in the cluster".to_string(),
                secondary: None,
            },
            ClusterHoprdEventEnum::NodeCreated => Event {
                type_: EventType::Normal,
                reason: "NodeCreated".to_string(),
                note: Some(format!("Node {} is created in the cluster", parsed_attribute)),
                action: "A new node is created in the cluster".to_string(),
                secondary: None,
            },
            ClusterHoprdEventEnum::DeletingNode => Event {
                type_: EventType::Normal,
                reason: "DeletingNode".to_string(),
                note: Some(format!("Node {} is being deleted from the cluster", parsed_attribute)),
                action: "Node is being deleted from the cluster".to_string(),
                secondary: None,
            },
            ClusterHoprdEventEnum::NodeDeleted => Event {
                type_: EventType::Normal,
                reason: "NodeDeleted".to_string(),
                note: Some(format!("Node {} is deleted from the cluster", parsed_attribute)),
                action: "Node is deleted from the cluster".to_string(),
                secondary: None,
            },
            ClusterHoprdEventEnum::Modified => Event {
                type_: EventType::Normal,
                reason: "Modified".to_string(),
                note: Some("ClusterHoprd configuration change detected".to_owned()),
                action: "ClusterHoprd reconfigured".to_string(),
                secondary: None,
            },
        }
    }
}

pub enum IdentityHoprdEventEnum {
    Initialized,
    Failed,
    Ready,
    InUse,
    Deleting,
}

impl ResourceEvent for IdentityHoprdEventEnum {
    fn to_event(&self, attribute: Option<String>) -> Event {
        let parsed_attribute = unwrap_attribute(&attribute);
        match self {
            IdentityHoprdEventEnum::Initialized => Event {
                type_: EventType::Normal,
                reason: "Initialized".to_string(),
                note: Some("Starting the process of creating a new identity".to_string()),
                action: "Initialized node identity".to_owned(),
                secondary: None,
            },
            IdentityHoprdEventEnum::Failed => Event {
                type_: EventType::Warning,
                reason: "Failed".to_string(),
                note: Some(format!("Failed to bootstrap identity {}", parsed_attribute)),
                action: "Identity bootstrapping failed".to_string(),
                secondary: None,
            },
            IdentityHoprdEventEnum::Ready => Event {
                type_: EventType::Normal,
                reason: "Ready".to_string(),
                note: Some("Identity is ready to be used by a Hoprd node".to_string()),
                action: "Identity ready to be used".to_owned(),
                secondary: None,
            },
            IdentityHoprdEventEnum::InUse => Event {
                type_: EventType::Normal,
                reason: "InUse".to_string(),
                note: Some(format!("Identity being used by Hoprd node {}", parsed_attribute)),
                action: "Identity is being used".to_string(),
                secondary: None,
            },
            IdentityHoprdEventEnum::Deleting => Event {
                type_: EventType::Normal,
                reason: "Deleting".to_string(),
                note: Some("Identity is being deleted".to_owned()),
                action: "Identity deletion started".to_string(),
                secondary: None,
            },
        }
    }
}

pub enum IdentityPoolEventEnum {
    Initialized,
    Failed,
    Ready,
    Deleting,
    Locked,
    Unlocked,
    IdentityCreated,
    IdentityDeleted,
}

impl ResourceEvent for IdentityPoolEventEnum {
    fn to_event(&self, attribute: Option<String>) -> Event {
        let parsed_attribute = unwrap_attribute(&attribute);
        match self {
            IdentityPoolEventEnum::Initialized => Event {
                type_: EventType::Normal,
                reason: "Initialized".to_string(),
                note: Some("Initializing identity pool".to_owned()),
                action: "Starting the process of initializing the identity pool".to_string(),
                secondary: None,
            },
            IdentityPoolEventEnum::Failed => Event {
                type_: EventType::Warning,
                reason: "Failed".to_string(),
                note: Some("Failed to bootstrap identity pool".to_owned()),
                action: "Identity pool bootstrap validations have failed".to_string(),
                secondary: None,
            },
            IdentityPoolEventEnum::Ready => Event {
                type_: EventType::Normal,
                reason: "Ready".to_string(),
                note: Some("Identity pool ready to be used".to_owned()),
                action: "Identity pool is ready to be used by a Hoprd node".to_string(),
                secondary: None,
            },
            IdentityPoolEventEnum::Deleting => Event {
                type_: EventType::Normal,
                reason: "Deleting".to_string(),
                note: Some("Identity pool is being deleted".to_owned()),
                action: "Identity pool deletion started".to_string(),
                secondary: None,
            },
            IdentityPoolEventEnum::Locked => Event {
                type_: EventType::Normal,
                reason: "Locked".to_string(),
                note: Some(format!("Identity {} locked from pool", parsed_attribute)),
                action: "Identity locking operation completed".to_string(),
                secondary: None,
            },
            IdentityPoolEventEnum::Unlocked => Event {
                type_: EventType::Normal,
                reason: "Unlocked".to_string(),
                note: Some(format!("Identity {} unlocked from pool", parsed_attribute)),
                action: "Identity unlocked from pool".to_string(),
                secondary: None,
            },
            IdentityPoolEventEnum::IdentityCreated => Event {
                type_: EventType::Normal,
                reason: "IdentityCreated".to_string(),
                note: Some(format!("Identity pool created identity {}", parsed_attribute)),
                action: "Identity pool created identity".to_string(),
                secondary: None,
            },
            IdentityPoolEventEnum::IdentityDeleted => Event {
                type_: EventType::Normal,
                reason: "IdentityDeleted".to_string(),
                note: Some(format!("Identity pool deregistered identity {}", parsed_attribute)),
                action: "Identity pool deregistered identity".to_string(),
                secondary: None,
            },
        }
    }
}
