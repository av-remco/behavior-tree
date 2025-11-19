use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use simple_xml_builder::XMLElement;
use std::fs::File;

use tokio::sync::mpsc::{channel, Receiver, Sender};

use handle::{ChildMessage, NodeError, NodeHandle, ParentMessage, Status};
use listener::Update;

use crate::bt::listener::Listener;
#[cfg(feature = "websocket")]
use crate::ws::socket_connector::SocketConnector;

const CHANNEL_SIZE: usize = 20;

pub mod action;
pub mod condition;
pub mod fallback;
pub mod handle;
pub mod listener;
pub mod sequence;

pub struct BehaviorTree {
    pub name: String,
    root_node: NodeHandle,
    handles: Vec<NodeHandle>,
    tx: Sender<Update>,
    rx: Option<Receiver<Update>>,
    status: Status,
}

impl BehaviorTree {
    pub fn new<S: Into<String>>(root_node: NodeHandle, name: S) -> Self {
        BehaviorTree::_new(root_node, name.into())
    }

    #[cfg(test)]
    pub fn new_test(root_node: NodeHandle) -> Self {
        BehaviorTree::_new(root_node, "test-tree".to_string())
    }

    fn _new(mut root_node: NodeHandle, name: String) -> Self {
        let handles = root_node.take_handles();
        // Non-unique UUIDs should not be possible, but check anyway
        BehaviorTree::verify_unique_ids(&handles).expect("UUIDs are non-unique!");
        let (tx, rx) = channel(CHANNEL_SIZE);
        Self {
            name,
            root_node,
            handles,
            tx,
            rx: Some(rx),
            status: Status::Idle,
        }
    }

    // Mutually exclusive public access to take rx.
    // Taking the rx is only allowed when handled externally, as opposed to the interal ws feature
    #[cfg(not(feature = "websocket"))]
    pub fn take_rx(&mut self) -> Result<Receiver<Update>> {
        self._take_rx()
    }

    #[cfg(feature = "websocket")]
    fn take_rx(&mut self) -> Result<Receiver<Update>> {
        self._take_rx()
    }

    fn _take_rx(&mut self) -> Result<Receiver<Update>> {
        self.rx.take().ok_or(anyhow!("Receiver already taken"))
    }

    // The connect function automatically send the BT once as soon as connection established
    // When connected, the writer forwards any updates directly
    #[cfg(feature = "websocket")]
    pub fn connect<S: Into<String>>(&mut self, socket_url: S) -> Result<()> {
        let rx = self.take_rx()?;
        let bt_export = self.export_json(self.name.clone())?;
        SocketConnector::spawn(socket_url.into(), rx, bt_export)?;
        Ok(())
    }

    // Execute the BT.
    // Upon Success resp. Failure, kills the tree and returns true resp. false
    pub async fn execute(&mut self) -> Result<bool, NodeError> {
        let mut listener: Listener =
            Listener::new(self.name.clone(), self.handles.clone(), self.tx.clone());
        tokio::spawn(async move { listener.run_listeners().await });
        self.root_node.send(ChildMessage::Start)?;
        log::debug!("Root - notify child {:?}: {:?}", self.root_node.name, ChildMessage::Start);
        self.status = Status::Running;
        loop {
            match self.root_node.listen().await? {
                ParentMessage::Status(status) => match status {
                    Status::Success => {
                        self.status = Status::Success;
                        log::debug!("Killing all handles");
                        self.kill().await;
                        return Ok(true) },
                    Status::Failure => {
                        self.status = Status::Failure;
                        log::debug!("Killing all handles");
                        self.kill().await;
                        return Ok(false) },
                    _ => {}
                },
                ParentMessage::RequestStart => panic!("Invalid message"),
                ParentMessage::Poison(err) => return Err(err),
                ParentMessage::Killed => return Err(NodeError::KillError), // This should not occur
            }
        }
    }

    pub async fn kill(&mut self) {
        for handle in &mut self.handles {
            log::debug!("Killing {} {:?}", handle.element, handle.name);
            handle.kill().await;
        }
        log::debug!("Killed all nodes succesfully");
    }

    fn verify_unique_ids(handles: &Vec<NodeHandle>) -> Result<()> {
        let mut ids = vec![];
        for handle in handles {
            ids.push(handle.id.clone());
        }
        let original_len = ids.len();
        ids.dedup();
        if ids.len() < original_len {
            Err(anyhow!("The behavior tree contained non-unique IDs"))
        } else {
            Ok(())
        }
    }

    pub fn save_xml_export<S: Into<String> + Clone>(&mut self, name: S) -> Result<()> {
        let file = File::create(format!("{}.xml", name.clone().into()))?;
        let root = self.export_xml(name)?;
        root.write(file)?;
        Ok(())
    }

    pub fn export_xml<S: Into<String> + Clone>(&mut self, name: S) -> Result<XMLElement> {
        // Groot format. See https://github.com/BehaviorTree/Groot
        let mut root = XMLElement::new("root");
        root.add_attribute("main_tree_to_execute", "MainTree");
        let mut tree = XMLElement::new(name.into());
        tree.add_attribute("ID", "MainTree");

        // Start with root node
        let root_element = self.root_node.get_xml();
        let children_names = self.root_node.children_names.clone();
        let root_element = self.add_children(&self.handles, root_element, children_names);

        tree.add_child(root_element); // Insert custom BT logic
        root.add_child(tree); // Insert in boilerplate

        Ok(root)
    }

    fn add_children(
        &self,
        handles: &Vec<NodeHandle>,
        mut element: XMLElement,
        children_names: Vec<String>,
    ) -> XMLElement {
        for child in &children_names {
            let handle = handles
                .iter()
                .find_map(|x| {
                    if x.name == *child {
                        Some(x.clone())
                    } else {
                        None
                    }
                })
                .expect("A child was not present in the handles!");

            let el_base = handle.get_xml();
            let children_names = handle.children_names.clone();
            let el_expanded = self.add_children(handles, el_base, children_names);
            element.add_child(el_expanded)
        }
        element
    }

    pub fn save_json_export<S: Into<String> + Clone>(&mut self, name: S) -> Result<()> {
        let file = File::create(format!("{}.json", name.clone().into()))?;
        let bt = self.export_json(name)?;
        serde_json::to_writer(&file, &bt)?;
        Ok(())
    }

    pub fn export_json<S: Into<String> + Clone>(&mut self, name: S) -> Result<Value> {
        let node_description: Vec<serde_json::value::Value> =
            self.handles.iter().map(|x| x.get_json()).collect();

        let bt = json!({
            "name": name.into(),
            "rootNode": self.root_node.id.clone(),
            "nodes": json!(node_description),
        });

        Ok(bt)
    }
}