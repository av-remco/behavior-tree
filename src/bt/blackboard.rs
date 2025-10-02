use std::{any::Any, collections::HashMap, sync::Arc};
use actify::Handle;
use dyn_clone::DynClone;


trait BbValue: DynClone + Send + Sync + Any {}

impl<T> BbValue for T where T: DynClone + Send + Sync + Any {}

pub struct Blackboard {
    bb: Arc<HashMap<String, Handle<Box<dyn BbValue>>>>,
}

impl Blackboard {
    pub fn new() -> Self {
        Self { bb: Arc::new(HashMap::new()) }
    }

    pub fn get(&self, key: String) -> Option<&Handle<Box<dyn BbValue>>> {
        self.bb.get(&key)
    }

    pub fn insert(&mut self, key: String, value: Handle<Box<dyn BbValue>>) {
        self.bb.insert(key, value);
    }
}

#[cfg(test)]
#[allow(unused_imports)]
mod tests {
    use crate::Blackboard;
    use actify::Handle;

    #[tokio::test]
    async fn test_read_from_bb() {
        let (key, value) = ("key", true);
        let bb = Blackboard::new();
        bb.insert(key.to_string(), Handle::new(Box::new(value)));
        let res = bb.get(&key.to_string()).unwrap().get().await;
        let res_bool = res.as_ref().as_any();
        assert!(*res)
    }

    #[tokio::test]
    async fn test_overwrite_bb_entry() {
        let (key1, value1) = ("key", false);
        let (key2, value2) = ("key", true);
        let bb = Blackboard::new();
        bb.insert(key1, value1);
        bb.insert(key2, value2);
        let res = bb.get(key1);
        assert!(res)
    }
}