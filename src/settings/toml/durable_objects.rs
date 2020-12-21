use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct DurableObjects {
    pub implements: Option<Vec<DurableObjectNamespaceImpl>>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct DurableObjectNamespaceImpl {
    pub class_name: String,
}
