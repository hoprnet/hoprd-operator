use crate::constants;
use std::collections::BTreeMap;

pub fn common_lables(name: String, instance: Option<String>, component: Option<String>) -> BTreeMap<String, String> {
    let mut labels: BTreeMap<String, String> = BTreeMap::new();
    labels.insert(constants::LABEL_KUBERNETES_NAME.to_owned(), name);
    match instance {
        Some(instance) => {
            labels.insert(constants::LABEL_KUBERNETES_INSTANCE.to_owned(), instance);
        }
        None => {}
    }
    match component {
        Some(component) => {
            labels.insert(constants::LABEL_KUBERNETES_COMPONENT.to_owned(), component);
        }
        None => {}
    }
    return labels;
}
