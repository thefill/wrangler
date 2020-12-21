mod durable_objects;
mod schedule;
mod zoned;
mod zoneless;

pub use durable_objects::DurableObjectNSTarget;
pub use schedule::ScheduleTarget;
pub use zoned::ZonedTarget;
pub use zoneless::ZonelessTarget;

use crate::settings::global_user::GlobalUser;

/// A set of deploy targets.
pub type DeploymentSet = Vec<DeployTarget>;

#[derive(Debug, PartialEq, Clone)]
pub enum DeployTarget {
    Zoned(ZonedTarget),
    Zoneless(ZonelessTarget),
    Schedule(ScheduleTarget),
    DurableObjectNamespace(DurableObjectNSTarget),
}

pub fn worker(
    user: &GlobalUser,
    deploy_targets: &[DeployTarget],
) -> Result<DeployResults, failure::Error> {
    let mut results = DeployResults::default();
    for target in deploy_targets {
        match target {
            DeployTarget::Zoned(zoned) => {
                let route_urls = zoned.deploy(user)?;
                results.urls.extend(route_urls);
            }
            DeployTarget::Zoneless(zoneless) => {
                let worker_dev = zoneless.deploy(user)?;
                results.urls.push(worker_dev);
            }
            DeployTarget::Schedule(schedule) => {
                let schedules = schedule.deploy(user)?;
                results.schedules.extend(schedules);
            }
            DeployTarget::DurableObjectNamespace(namespace) => {
                let namespace = namespace.deploy(user)?;
                results.durable_object_namespaces.push(namespace);
            }
        }
    }

    Ok(results)
}

#[derive(Default)]
pub struct DeployResults {
    pub urls: Vec<String>,
    pub schedules: Vec<String>,
    pub durable_object_namespaces: Vec<String>,
}
