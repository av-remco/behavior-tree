use std::result;

use log::warn;

use crate::{BehaviorTree, NodeHandle, Status};

#[derive(Debug, Clone, PartialEq)]
pub struct Trace {
    trace: Vec<(String, Status)>,
    result: Status,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Traces {
    traces: Vec<Trace>,
}

pub fn get_traces(bt: BehaviorTree) -> Traces {
    rec(bt.root_node, &bt.handles)
}

pub fn log_traces(traces: Traces) {
    println!("--- Behavior Tree Traces ---");
    for trace in traces.traces {
        print_path(trace);
    }
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

fn append_traces(trace1: Trace, traces2: Traces) -> Vec<Trace> {
    let mut res = vec![];

    for trace2 in traces2.traces.iter() {
        let mut combined_trace = trace1.trace.clone();
        combined_trace.extend(trace2.trace.clone());

        res.push(Trace {
            trace: combined_trace,
            result: trace2.result.clone(),
        });
    }

    res
}


fn rec(node: NodeHandle, handles: &Vec<NodeHandle>) -> Traces {
    match node.element.as_str() {
        "Action" => {
            Traces { traces: vec![
                Trace {
                    trace: vec![(node.name.clone(), Status::Success)],
                    result: Status::Success,
                },
                Trace {
                    trace: vec![(node.name.clone(), Status::Failure)],
                    result: Status::Failure,
                },
                Trace {
                    trace: vec![(node.name.clone(), Status::Running)],
                    result: Status::Running,
                }
            ] }
        },
        "Condition" => {
            // Conditions/decorators have exactly one child
            let mut new_traces = Traces { traces: vec![] };
            let children = get_children_handles(handles, node.children_names.clone());;
            if let Some(child) = children.first() {
                let child_traces = rec(child.clone(), handles);
                for child_trace in child_traces.traces {
                    let mut new_trace = vec![(node.name.clone(), Status::Success)];
                    new_trace.append(&mut child_trace.trace.clone());
                    new_traces.traces.push(Trace { trace: new_trace, result: child_trace.result });
                }
                new_traces.traces.push(Trace { trace: vec![(node.name.clone(), Status::Failure)], result: Status::Failure });
            } else {
                println!("Decorator '{}' has no child!", node.name);
            }
            new_traces
        },
        "Sequence" => {
            let mut new_traces = Traces { traces: vec![] };
            let children = get_children_handles(handles, node.children_names.clone());
            new_traces.traces = rec_sequence(children.clone(), vec![], vec![], handles).traces;
            new_traces
        },
        other => {
            println!("Unknown element type '{}'", other);
            Traces { traces: vec![] }
        },
    }
}

fn rec_sequence(mut children: Vec<NodeHandle>, mut prepend: Vec<Trace>, mut final_traces: Vec<Trace>, handles: &Vec<NodeHandle>) -> Traces {
    println!("Children: {:?}", children);
    println!("prepend: {:?}", prepend);
    println!("final_traces: {:?}", final_traces);
    if let Some(child) = children.pop() {
        let mut new_prepend = vec![];
        let child_traces = rec(child.clone(), handles);
        for child_trace in child_traces.traces {
            match child_trace.result.clone() {
                Status::Failure => final_traces.push(child_trace),
                Status::Running => final_traces.push(child_trace),
                Status::Success => {
                    if prepend.is_empty() {
                        new_prepend.push(child_trace);
                    } else {
                        let mut combined_traces: Vec<_> = prepend
                            .iter()
                            .map(|x| {
                                let mut y = x.clone();
                                y.trace.extend(child_trace.trace.clone());
                                y
                            })
                            .collect();

                        new_prepend.append(&mut combined_traces);
                    }
                },
                other => {
                    println!("Unknown element type '{:?}'", other);
                }
            }
        }
        rec_sequence(children, new_prepend, final_traces, handles)
    } else {
        final_traces.append(&mut prepend);
        Traces { traces: final_traces }
    }
}

/// Pretty-print a single trace path
fn print_path(trace: Trace) {
    let result = trace.result;
    let path = trace.trace;
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

    use crate::{BehaviorTree, Condition, Sequence, Status, bt::{action::mocking::MockAction, paths::{get_traces, log_traces}}};

    #[tokio::test]
    async fn test_action_trace() {
        let action = MockAction::new(1);
        let bt = BehaviorTree::new_test(action.clone());

        log_traces(get_traces(bt));
    }

    #[tokio::test]
    async fn test_condition_trace() {
        let handle = Handle::new(1);
        let action = MockAction::new(1);
        let con = Condition::new("C", handle, |x| x > 0, action);
        let bt = BehaviorTree::new_test(con.clone());

        log_traces(get_traces(bt));
    }

    #[tokio::test]
    async fn test_sequence_trace() {
        let action1 = MockAction::new(1);
        let action2 = MockAction::new(2);
        let sq = Sequence::new(vec![action1.clone(), action2.clone()]);
        let bt = BehaviorTree::new_test(sq.clone());

        log_traces(get_traces(bt));
    }

}