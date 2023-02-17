use std::collections::BTreeMap;
use crate::constants;

pub fn common_lables(instance_name: &String) -> BTreeMap<String, String> {
    let mut labels: BTreeMap<String, String> = BTreeMap::new();
    labels.insert(constants::LABEL_KUBERNETES_NAME.to_owned(), "hoprd".to_owned());
    labels.insert(constants::LABEL_KUBERNETES_INSTANCE.to_owned(), instance_name.to_owned());
    return labels;
}

pub fn get_hopr_image_tag(tag: &String) -> String {
    let mut image = String::from(constants::HOPR_DOCKER_REGISTRY.to_owned());
    image.push_str("/");
    image.push_str(constants::HOPR_DOCKER_IMAGE_NAME);
    image.push_str(":");
    image.push_str(&tag.to_owned());
    return image;
}