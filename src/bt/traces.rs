use crate::{BehaviorTree, NodeHandle, Status};

pub fn log_traces(bt: BehaviorTree) {
    println!("--- Behavior Tree Traces ---");
    rec(bt.root_node, Vec::new(), &bt.handles);
    println!("-----------------------------");
}

fn get_children_handles(handles: &Vec<NodeHandle>, children_names: Vec<String>) -> Vec<NodeHandle> {
    children_names
        .into_iter()
        .map(|child| {
            handles
                .iter()
                .find(|x| x.name == child)
                .cloned()
                .expect("A child was not present in the handles!")
        })
        .collect()
}


fn rec(node: NodeHandle, mut path: Vec<(String, Status)>, handles: &Vec<NodeHandle>) {
    println!("Matching: {}", node.element.as_str());
    match node.element.as_str() {
        "Action" => {
            // For each possible result, print a line
            for result in [Status::Success, Status::Failure] {
                let mut new_path = path.clone();
                new_path.push((node.name.clone(), result.clone()));
                print_path(&new_path, result);
            }
        }
        "Sequence" => {
            let children = get_children_handles(handles, node.children_names.clone());
            // Recursively explore all combinations
            // rec_sequence(children.as_slice(), path);
        }
        "Decorator" | "Condition" => {
            // Conditions/decorators have exactly one child
            let children = get_children_handles(handles, node.children_names.clone());;
            if let Some(child) = children.first() {
                path.push((node.name.clone(), Status::Failure));
                print_path(&new_path, Status::Failure);

                // Add the decorator itself in the path
                path.push((node.name.clone(), Status::Success));
                rec(child.clone(), path, handles);
            } else {
                println!("Decorator '{}' has no child!", node.name);
            }
        }
        other => {
            println!("Unknown element type '{}'", other);
        }
    }
}


/// Pretty-print a single trace path
fn print_path(path: &[(String, Status)], result: Status) {
    let text = path
        .iter()
        .map(|(name, status)| format!("{}({:?})", name, status))
        .collect::<Vec<_>>()
        .join(" -> ");
    println!("[{}] => Result: {:?}", text, result);
}

#[cfg(test)]
#[allow(unused_imports)]
mod tests {
    use actify::Handle;

    use crate::{BehaviorTree, Condition, Sequence, Status, bt::{action::mocking::MockAction, traces::log_traces}};

    #[tokio::test]
    async fn test_action_trace() {
        let action = MockAction::new(1);
        let bt = BehaviorTree::new_test(action.clone());

        log_traces(bt);
    }

    #[tokio::test]
    async fn test_condition_trace() {
        let handle = Handle::new(1);
        let action = MockAction::new(1);
        let con = Condition::new("C", handle, |x| x > 0, action);
        let bt = BehaviorTree::new_test(con.clone());

        log_traces(bt);
    }

    #[tokio::test]
    async fn test_sequence_trace() {
        let action1 = MockAction::new(1);
        let action2 = MockAction::new(2);
        let sq = Sequence::new(vec![action1.clone(), action2.clone()]);
        let bt = BehaviorTree::new_test(sq.clone());

        log_traces(bt);
    }

}