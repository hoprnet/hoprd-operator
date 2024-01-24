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

impl ResourceEvent for HoprdEventEnum {

    fn to_event(&self, _: Option<String>) -> Event {
        match self {
            HoprdEventEnum::Initializing => Event {
                type_: EventType::Normal,
                reason: "Initializing".to_string(),
                note: Some("Initializing Hoprd node".to_owned()),
                action: "Starting the process of creating a new node".to_string(),
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
}

impl ResourceEvent for ClusterHoprdEventEnum {
    fn to_event(&self, attribute: Option<String>) -> Event {
        match self {
            ClusterHoprdEventEnum::Initialized => Event {
                type_: EventType::Normal,
                reason: "Initialized".to_string(),
                note: Some("ClusterHoprd node initialized".to_owned()),
                action: "Starting the process of creating a new cluster of hoprd".to_string(),
                secondary: None,
            },
            ClusterHoprdEventEnum::NotScaled => Event {
                type_: EventType::Warning,
                reason: "NotScaled".to_string(),
                note: Some(format!("ClusterHoprd is not scaled. There are {} nodes pending to be synchronized", attribute.as_ref().unwrap_or(&"unknown".to_string()))),
                action: "ClusterHoprd is not scaled".to_string(),
                secondary: None,
            },
            ClusterHoprdEventEnum::Scaling => Event {
                type_: EventType::Warning,
                reason: "Scaling".to_string(),
                note: Some("ClusterHoprd is scaling".to_string()),
                action: "ClusterHoprd is scaling to meet the required replicas".to_string(),
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
                action: "ClusterHoprd is in ready phase".to_string(),
                secondary: None,
            },
            ClusterHoprdEventEnum::Deleting => Event {
                type_: EventType::Normal,
                reason: "Deleting".to_string(),
                note: Some("ClusterHoprd is being deleted".to_owned()),
                action: "ClusterHoprd is being deleted".to_string(),
                secondary: None,
            },
            ClusterHoprdEventEnum::CreatingNode => Event {
                type_: EventType::Normal,
                reason: "CreatingNode".to_string(),
                note: Some(format!("Node {} is being created in the cluster", attribute.as_ref().unwrap_or(&"unknown".to_string()))),
                action: "A new node is being created in the cluster".to_string(),
                secondary: None,
            },
            ClusterHoprdEventEnum::NodeCreated => Event {
                type_: EventType::Normal,
                reason: "NodeCreated".to_string(),
                note: Some(format!("Node {} is created in the cluster", attribute.as_ref().unwrap_or(&"unknown".to_string()))),
                action: "A new node is created in the cluster".to_string(),
                secondary: None,
            },
            ClusterHoprdEventEnum::DeletingNode => Event {
                type_: EventType::Normal,
                reason: "DeletingNode".to_string(),
                note: Some(format!("Node {} is being deleted from the cluster", attribute.as_ref().unwrap_or(&"unknown".to_string()))),
                action: format!("Node {} is being deleted from the cluster", attribute.as_ref().unwrap_or(&"unknown".to_string())),
                secondary: None,
            },
            ClusterHoprdEventEnum::NodeDeleted => Event {
                type_: EventType::Normal,
                reason: "NodeDeleted".to_string(),
                note: Some(format!("Node {} is deleted from the cluster", attribute.as_ref().unwrap_or(&"unknown".to_string()))),
                action: format!("Node {} is deleted from the cluster", attribute.as_ref().unwrap_or(&"unknown".to_string())),
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
        match self {
            IdentityHoprdEventEnum::Initialized => Event {
                type_: EventType::Normal,
                reason: "Initialized".to_string(),
                note: Some("Initialized node identity".to_owned()),
                action: "Starting the process of creating a new identity".to_string(),
                secondary: None,
            },
            IdentityHoprdEventEnum::Failed => Event {
                type_: EventType::Warning,
                reason: "Failed".to_string(),
                note: Some("Failed to bootstrap identity".to_owned()),
                action: "Identity bootstrapping failed".to_string(),
                secondary: None,
            },
            IdentityHoprdEventEnum::Ready => Event {
                type_: EventType::Normal,
                reason: "Ready".to_string(),
                note: Some("Identity ready to be used".to_owned()),
                action: "Identity is ready to be used by a Hoprd node".to_string(),
                secondary: None,
            },
            IdentityHoprdEventEnum::InUse => Event {
                type_: EventType::Normal,
                reason: "InUse".to_string(),
                note: Some(format!("Identity being used by Hoprd node {}", attribute.unwrap_or("unknown".to_owned()))),
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
    OutOfSync,
    Ready,
    Deleting,
    Locked,
    Unlocked,
    CreatingIdentity,
    IdentityCreated,
    IdentityDeleted,
}

impl ResourceEvent for IdentityPoolEventEnum {
    fn to_event(&self, attribute: Option<String>) -> Event {
        match self {
            IdentityPoolEventEnum::Initialized => Event {
                        type_: EventType::Normal,
                        reason: "Initialized".to_string(),
                        note: Some("Initializing identity pool".to_owned()),
                        action: "The service monitor has been created".to_string(),
                        secondary: None
                    },
            IdentityPoolEventEnum::Failed => Event {
                        type_: EventType::Warning,
                        reason: "Failed".to_string(),
                        note: Some("Failed to bootstrap identity pool".to_owned()),
                        action: "Identity pool bootstrap validations have failed".to_string(),
                        secondary: None
                    },
            IdentityPoolEventEnum::OutOfSync => Event {
                        type_: EventType::Normal,
                        reason: "OutOfSync".to_string(),
                        note: Some(format!("The identity pool is out of sync. There are {} identities pending to be created", attribute.unwrap_or("unknown".to_owned()))),
                        action: "The identity pool need to create more identities".to_string(),
                        secondary: None
                    },
            IdentityPoolEventEnum::Ready => Event {
                        type_: EventType::Normal,
                        reason: "Ready".to_string(),
                        note: Some("Identity pool ready to be used".to_owned()),
                        action: "Identity pool is ready to be used by a Hoprd node".to_string(),
                        secondary: None
                    },
            IdentityPoolEventEnum::Deleting => Event {
                        type_: EventType::Normal,
                        reason: "Deleting".to_string(),
                        note: Some("Identity pool is being deleted".to_owned()),
                        action: "Identity pool deletion started".to_string(),
                        secondary: None
            },
            IdentityPoolEventEnum::Locked => Event {
                        type_: EventType::Normal,
                        reason: "Locked".to_string(),
                        note: Some(format!("Identity {} locked from pool", attribute.as_ref().unwrap_or(&"unknown".to_string()))),
                        action: format!("Identity {} locked from pool", attribute.as_ref().unwrap_or(&"unknown".to_string())),
                        secondary: None
                    },
            IdentityPoolEventEnum::Unlocked => Event {
                        type_: EventType::Normal,
                        reason: "Unlocked".to_string(),
                        note: Some(format!("Identity {} unlocked from pool", attribute.as_ref().unwrap_or(&"unknown".to_string()))),
                        action: format!("Identity {} unlocked from pool", attribute.as_ref().unwrap_or(&"unknown".to_string())),
                        secondary: None
                    },
            IdentityPoolEventEnum::CreatingIdentity => Event {
                        type_: EventType::Normal,
                        reason: "CreatingIdentity".to_string(),
                        note: Some(format!("Creating identity {}", attribute.as_ref().unwrap_or(&"unknown".to_string()))),
                        action: format!("Creating identity {}", attribute.as_ref().unwrap_or(&"unknown".to_string())),
                        secondary: None
                    },
            IdentityPoolEventEnum::IdentityCreated => Event {
                        type_: EventType::Normal,
                        reason: "IdentityCreated".to_string(),
                        note: Some(format!("Identity pool created identity {}", attribute.as_ref().unwrap_or(&"unknown".to_string()))),
                        action: format!("Identity pool created identity {}", attribute.as_ref().unwrap_or(&"unknown".to_string())),
                        secondary: None
                    },
            IdentityPoolEventEnum::IdentityDeleted => Event {
                        type_: EventType::Normal,
                        reason: "IdentityDeleted".to_string(),
                        note: Some(format!("Identity pool deregistered identity {}", attribute.as_ref().unwrap_or(&"unknown".to_string()))),
                        action: format!("Identity pool deregistered identity {}", attribute.as_ref().unwrap_or(&"unknown".to_string())),
                        secondary: None
                    }
        }
    }
}

