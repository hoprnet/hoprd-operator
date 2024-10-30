use k8s_openapi::api::core::v1::ServiceAccount;
use k8s_openapi::api::rbac::v1::{PolicyRule, Role, RoleBinding, RoleRef, Subject};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
use kube::api::{DeleteParams, PostParams};
use kube::core::ObjectMeta;
use kube::runtime::wait::{await_condition, conditions};
use kube::{Api, Client};
use std::sync::Arc;
use tracing::{error, info};

use crate::context_data::ContextData;

use crate::model::Error;
use crate::utils;

pub async fn create_rbac(context_data: Arc<ContextData>, namespace: &String, name: &String, owner_references: Option<Vec<OwnerReference>>) -> Result<(), Error> {
    create_service_account(context_data.clone(), namespace, name, owner_references.to_owned()).await.unwrap();
    create_role(context_data.clone(), namespace, name, owner_references.to_owned()).await.unwrap();
    create_role_binding(context_data.clone(), namespace, name, owner_references.to_owned()).await.unwrap();
    Ok(())
}

pub async fn delete_rbac(client: Client, namespace: &str, name: &str) -> Result<(), Error> {
    delete_service_account(client.clone(), namespace, name).await?;
    delete_role(client.clone(), namespace, name).await?;
    delete_role_binding(client.clone(), namespace, name).await?;
    Ok(())
}

/// Creates a new service Account for the IdentityPool
async fn create_service_account(context_data: Arc<ContextData>, namespace: &String, name: &String, owner_references: Option<Vec<OwnerReference>>) -> Result<ServiceAccount, Error> {
    let labels = utils::common_lables(context_data.config.instance.name.to_owned(), Some(name.to_owned()), None);
    let api: Api<ServiceAccount> = Api::namespaced(context_data.client.clone(), namespace);
    let service_account: ServiceAccount = ServiceAccount {
        metadata: ObjectMeta {
            labels: Some(labels.clone()),
            name: Some(name.to_owned()),
            namespace: Some(namespace.to_owned()),
            owner_references,
            ..ObjectMeta::default()
        },
        ..ServiceAccount::default()
    };
    match api.create(&PostParams::default(), &service_account).await {
        Ok(sa) => Ok(sa),
        Err(error) => {
            error!("Could not create ServiceAccount {:?}", error);
            Err(Error::HoprdConfigError(format!("Could not create ServiceAccount for {} in namespace {}.", name, namespace)))
        }
    }
}

async fn create_role(context_data: Arc<ContextData>, namespace: &String, name: &String, owner_references: Option<Vec<OwnerReference>>) -> Result<Role, Error> {
    let labels = utils::common_lables(context_data.config.instance.name.to_owned(), Some(name.to_owned()), None);
    let api: Api<Role> = Api::namespaced(context_data.client.clone(), namespace);
    let role: Role = Role {
        metadata: ObjectMeta {
            labels: Some(labels.clone()),
            name: Some(name.to_owned()),
            namespace: Some(namespace.to_owned()),
            owner_references,
            ..ObjectMeta::default()
        },
        rules: Some(vec![PolicyRule {
            api_groups: Some(vec!["hoprnet.org".to_owned()]),
            resources: Some(vec!["identityhoprds".to_owned()]),
            verbs: vec!["get".to_owned(), "list".to_owned(), "watch".to_owned(), "create".to_owned(), "delete".to_owned()],
            ..PolicyRule::default()
        }]),
    };
    match api.create(&PostParams::default(), &role).await {
        Ok(role) => Ok(role),
        Err(error) => {
            error!("Could not create Role {:?}", error);
            Err(Error::HoprdConfigError(format!("Could not create Role for {} in namespace {}.", name, namespace)))
        }
    }
}

async fn create_role_binding(context_data: Arc<ContextData>, namespace: &String, name: &String, owner_references: Option<Vec<OwnerReference>>) -> Result<RoleBinding, Error> {
    let labels = utils::common_lables(context_data.config.instance.name.to_owned(), Some(name.to_owned()), None);
    let api: Api<RoleBinding> = Api::namespaced(context_data.client.clone(), namespace);
    let role_binding: RoleBinding = RoleBinding {
        metadata: ObjectMeta {
            labels: Some(labels.clone()),
            name: Some(name.to_owned()),
            namespace: Some(namespace.to_owned()),
            owner_references,
            ..ObjectMeta::default()
        },
        role_ref: RoleRef {
            name: name.to_owned(),
            kind: "Role".to_owned(),
            ..RoleRef::default()
        },
        subjects: Some(vec![Subject {
            name: name.to_owned(),
            kind: "ServiceAccount".to_owned(),
            ..Subject::default()
        }]),
    };
    match api.create(&PostParams::default(), &role_binding).await {
        Ok(rb) => Ok(rb),
        Err(error) => {
            error!("Could not create RoleBinding {:?}", error);
            Err(Error::HoprdConfigError(format!("Could not create RoleBinding for {} in namespace {}.", name, namespace)))
        }
    }
}

async fn delete_service_account(client: Client, name: &str, namespace: &str) -> Result<(), Error> {
    let api: Api<ServiceAccount> = Api::namespaced(client, namespace);
    if let Some(service_account) = api.get_opt(name).await? {
        let uid = service_account.metadata.uid.unwrap();
        api.delete(name, &DeleteParams::default()).await?;
        await_condition(api, name, conditions::is_deleted(&uid)).await.unwrap();
        Ok(info!("ServiceAccount {name} successfully deleted"))
    } else {
        Ok(info!("ServiceAccount {name} in namespace {namespace} about to delete not found"))
    }
}

async fn delete_role(client: Client, name: &str, namespace: &str) -> Result<(), Error> {
    let api: Api<Role> = Api::namespaced(client, namespace);
    if let Some(role) = api.get_opt(name).await? {
        let uid = role.metadata.uid.unwrap();
        api.delete(name, &DeleteParams::default()).await?;
        await_condition(api, name, conditions::is_deleted(&uid)).await.unwrap();
        Ok(info!("Role {name} successfully deleted"))
    } else {
        Ok(info!("Role {name} in namespace {namespace} about to delete not found"))
    }
}

async fn delete_role_binding(client: Client, name: &str, namespace: &str) -> Result<(), Error> {
    let api: Api<RoleBinding> = Api::namespaced(client, namespace);
    if let Some(role_binding) = api.get_opt(name).await? {
        let uid = role_binding.metadata.uid.unwrap();
        api.delete(name, &DeleteParams::default()).await?;
        await_condition(api, name, conditions::is_deleted(&uid)).await.unwrap();
        Ok(info!("RoleBinding {name} successfully deleted"))
    } else {
        Ok(info!("RoleBinding {name} in namespace {namespace} about to delete not found"))
    }
}
