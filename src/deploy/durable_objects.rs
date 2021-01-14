use std::{cell::RefCell, collections::HashMap};

use crate::settings::global_user::GlobalUser;
use crate::settings::toml::DurableObjects;
use crate::{http, settings::toml::Target};

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

/*
Durable Objects have a fun chicken and egg problem that makes them rather tricky to deploy,
in the case of scripts that both implement and bind to a namespace... you must have a namespace
id in order to create the binding to upload the script, but you need the script to create the
namespace. What we did to get around this, was allow creating a namespace without a script,
that just returns an error when you try to access it.

When you use/implement a DO in the same script, we'll first initialize the namespace (if it does
not exist already) with no script/class so we have an ID to use for the binding. After the script is
uploaded, the namespace is finalized with the script and class filled in.

Scripts using durable objects should be written to handle errors accessing the object to handle
the small race between the script being uploaded (and potentially receiving a request), and
the durable object namespace being properly deployed.

There's only a single DurableObjectsTarget even if there are multiple used durable object
namespaces, or multiple implemented durable object namespaces. This is because we must handle the
(quite common) special case where a single script both implements and uses a durable object
namespace, and it's much easier to handle the initialization/finalization dance if all of the data
necessary to do that is available in one struct.
*/

#[derive(Clone, Debug, PartialEq)]
pub struct DurableObjectsTarget {
    pub account_id: String,
    pub script_name: String,
    pub durable_objects: DurableObjects,
    existing_namespaces: RefCell<HashMap<String, ApiDurableObjectNSResponse>>,
}

#[derive(Serialize, Debug)]
struct DurableObjectCreateNSRequest {
    pub name: String,
    #[serde(flatten)]
    pub implementation: Option<DurableObjectNSImpl>,
}

#[derive(Serialize, Debug)]
struct DurableObjectNSImpl {
    pub script: String,
    pub class: String,
}

impl DurableObjectsTarget {
    pub fn new(account_id: String, script_name: String, durable_objects: DurableObjects) -> Self {
        Self {
            account_id,
            script_name,
            durable_objects,
            existing_namespaces: RefCell::new(HashMap::new()),
        }
    }

    pub fn pre_upload(&self, user: &GlobalUser, target: &mut Target) -> Result<(), failure::Error> {
        &self.get_existing_namespaces(user)?;
        &self.init_self_referential_namespaces(user)?;
        &self.hydrate_target_with_ns_ids(target)?;
        Ok(())
    }

    pub fn deploy(&self, user: &GlobalUser) -> Result<Vec<String>, failure::Error> {
        let existing_namespaces = self.existing_namespaces.borrow();
        let new_namespaces = self
            .durable_objects
            .implements
            .iter()
            .flat_map(|nses| nses)
            .filter_map(|ns| {
                if !existing_namespaces.contains_key(&ns.namespace_name) {
                    Some(ns)
                } else {
                    None
                }
            });

        let update_namespaces = self
            .durable_objects
            .implements
            .iter()
            .flat_map(|nses| nses)
            .filter_map(|ns| {
                if let Some(current) = existing_namespaces.get(&ns.namespace_name) {
                    if current.script.as_ref() != Some(&self.script_name)
                        || current.class.as_ref() != Some(&ns.class_name)
                    {
                        Some((current.id.clone(), ns))
                    } else {
                        None
                    }
                } else {
                    None
                }
            });

        let mut updated_namespaces = vec![];

        for ns in new_namespaces {
            updated_namespaces.push(ns.namespace_name.clone());
            create_ns(
                &DurableObjectCreateNSRequest {
                    name: ns.namespace_name.clone(),
                    implementation: Some(DurableObjectNSImpl {
                        script: self.script_name.clone(),
                        class: ns.class_name.clone(),
                    }),
                },
                &self.account_id,
                user,
            )?;
        }

        for (id, ns) in update_namespaces {
            updated_namespaces.push(ns.namespace_name.clone());
            update_ns(
                &id,
                &DurableObjectNSImpl {
                    script: self.script_name.clone(),
                    class: ns.class_name.clone(),
                },
                &self.account_id,
                user,
            )?;
        }

        Ok(updated_namespaces)
    }

    fn get_existing_namespaces(&self, user: &GlobalUser) -> Result<(), failure::Error> {
        log::info!("getting existing durable objects");
        let client = http::legacy_auth_client(user);
        self.existing_namespaces
            .replace(list_namespaces_by_name_to_id(&client, &self.account_id)?);
        Ok(())
    }

    fn init_self_referential_namespaces(&self, user: &GlobalUser) -> Result<(), failure::Error> {
        log::info!("initializing self-referential namespaces");
        let implemented_namespace_names = self
            .durable_objects
            .uses
            .iter()
            .flat_map(|nses| nses)
            .filter_map(|ns| ns.namespace_name.as_ref())
            .collect::<Vec<_>>();

        let existing_namespaces = self.existing_namespaces.borrow();
        let existing_namespace_names = existing_namespaces.keys().collect::<Vec<_>>();

        let new_self_referential_namespace_names = self
            .durable_objects
            .uses
            .iter()
            .flat_map(|nses| nses)
            .filter_map(|ns| {
                ns.namespace_name.as_ref().and_then(|name| {
                    if implemented_namespace_names.contains(&name)
                        && !existing_namespace_names.contains(&name)
                    {
                        Some(name)
                    } else {
                        None
                    }
                })
            });

        for name in new_self_referential_namespace_names {
            log::info!("creating error namespace {}", name);
            let new_ns = create_ns(
                &DurableObjectCreateNSRequest {
                    name: name.clone(),
                    implementation: None,
                },
                &self.account_id,
                user,
            )?;
            self.existing_namespaces
                .borrow_mut()
                .insert(new_ns.id.clone(), new_ns);
        }

        Ok(())
    }

    fn hydrate_target_with_ns_ids(&self, target: &mut Target) -> Result<(), failure::Error> {
        for ns in target.used_durable_object_namespaces.iter_mut() {
            if let Some(name) = &ns.namespace_name {
                if ns.namespace_id.is_none() {
                    ns.namespace_id = self
                        .existing_namespaces
                        .borrow()
                        .get(name)
                        .cloned()
                        .and_then(|ns| Some(ns.id));
                    if ns.namespace_id.is_none() {
                        failure::bail!(format!(
                            "durable object namespace with name {} was not found in your account.",
                            name,
                        ))
                    }
                }
            }
        }
        Ok(())
    }
}

fn create_ns(
    body: &DurableObjectCreateNSRequest,
    account_id: &str,
    user: &GlobalUser,
) -> Result<ApiDurableObjectNSResponse, failure::Error> {
    let durable_object_namespace_addr = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/workers/durable_objects/namespaces",
        account_id
    );

    let client = http::legacy_auth_client(user);
    let res = client
        .post(&durable_object_namespace_addr)
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(body)?)
        .send()?;

    if !res.status().is_success() {
        failure::bail!(
            "Something went wrong! Status: {}, Details {}, body: {:?}",
            res.status(),
            res.text()?,
            body
        )
    }

    match res.json::<ApiSingleDurableObjectNSResponse>()?.result {
        Some(result) => Ok(result),
        None => Err(failure::err_msg(
            "durable object not returned from create call despite success status",
        )),
    }
}

fn update_ns(
    namespace_id: &str,
    body: &DurableObjectNSImpl,
    account_id: &str,
    user: &GlobalUser,
) -> Result<(), failure::Error> {
    let durable_object_namespace_addr = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/workers/durable_objects/namespaces/{}",
        account_id, namespace_id
    );

    let client = http::legacy_auth_client(user);
    let res = client
        .put(&durable_object_namespace_addr)
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(body)?)
        .send()?;

    if !res.status().is_success() {
        failure::bail!(
            "Something went wrong! Status: {}, Details {}, body: {:?}",
            res.status(),
            res.text()?,
            body
        )
    }

    Ok(())
}

#[derive(Serialize, Clone, Deserialize, Debug, PartialEq)]
struct ApiDurableObjectNSResponse {
    pub name: String,
    pub id: String,
    pub script: Option<String>,
    pub class: Option<String>,
}

#[derive(Deserialize)]
struct ApiSingleDurableObjectNSResponse {
    pub result: Option<ApiDurableObjectNSResponse>,
}

#[derive(Deserialize)]
struct ApiListDurableObjectNSResponse {
    pub result: Option<Vec<ApiDurableObjectNSResponse>>,
}

fn list_namespaces_by_name_to_id(
    client: &Client,
    account_id: &str,
) -> Result<HashMap<String, ApiDurableObjectNSResponse>, failure::Error> {
    let mut map = HashMap::new();
    let namespaces = list_namespaces(client, account_id)?;
    for namespace in namespaces {
        map.insert(namespace.name.clone(), namespace);
    }

    Ok(map)
}

fn list_namespaces(
    client: &Client,
    account_id: &str,
) -> Result<Vec<ApiDurableObjectNSResponse>, failure::Error> {
    let list_addr = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/workers/durable_objects/namespaces",
        account_id,
    );

    let res = client
        .get(&list_addr)
        .header("Content-type", "application/json")
        .send()?;

    if !res.status().is_success() {
        failure::bail!(
            "Something went wrong! Status: {}, Details {}",
            res.status(),
            res.text()?
        )
    }

    Ok(res
        .json::<ApiListDurableObjectNSResponse>()?
        .result
        .unwrap_or_default())
}
