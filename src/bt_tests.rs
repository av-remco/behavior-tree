#[cfg(test)]
#[allow(unused_imports)]
mod tests {
    use actify::Handle;
    use tokio::sync::mpsc::Receiver;
    use crate::bt::handle::Status;
    use crate::{BehaviorTree, NodeError, NodeHandle, Update};
    use std::collections::HashMap;
    use tokio::time::{Duration, sleep};

    use crate::bt::*;
    use crate::bt::{
        action::{Failure, Success},
        condition::{Condition, OneTimeCondition},
        fallback::Fallback,
        sequence::Sequence,
    };
    use crate::bt::action::mocking::MockAction;
    use crate::bt::condition::mocking::MockAsyncCondition;
    use crate::logging::load_logger;
    use crate::{Wait};
    use listener::OuterStatus;
    use logtest::Logger;

    async fn dummy_bt() -> BehaviorTree {
        let handle = Handle::new(-1);
        let action1 = MockAction::new(1);
        let cond1 = Condition::new("cond1", handle.clone(), |i: i32| i > 0, action1);
        let seq = Sequence::new(vec![cond1]);
        let action2 = MockAction::new_failing(2);
        let fb = Fallback::new(vec![seq, action2]);
        BehaviorTree::new_test(fb)
    }

    #[tokio::test]
    #[ignore]
    async fn test_save_xml_export() {
        let mut bt = dummy_bt().await;
        let res = bt.save_xml_export("Test_BT");
        assert!(res.is_ok())
    }

    #[tokio::test]
    #[ignore]
    async fn test_save_json_export() {
        let mut bt = dummy_bt().await;
        let res = bt.save_json_export("Test_BT");
        assert!(res.is_ok())
    }

    #[tokio::test]
    async fn test_export_xml() {
        let mut bt = dummy_bt().await;
        let res = bt.export_xml("Test_BT");
        assert!(res.is_ok())
    }

    #[tokio::test]
    async fn test_export_json() {
        let mut bt = dummy_bt().await;
        let res = bt.export_json("Test_BT");
        assert!(res.is_ok())
    }

    #[cfg(feature = "websocket")]
    #[tokio::test]
    #[ignore]
    async fn test_websocket_connection() {
        let mut bt = dummy_bt().await;
        bt.connect(format!("ws://{}:{}", "localhost", 4012))
            .unwrap();
        sleep(Duration::from_secs(1)).await; // Allow for treabeard to receive messages

        // Enable this if you want to see updates
        // let _ = bt.run().await;
    }

    #[tokio::test]
    async fn test_killing_bt() {
        // Setup
        let handle = Handle::new(-1);

        // When
        let action = MockAction::new(1);
        let cond = Condition::new("1", handle.clone(), |i: i32| i > 0, action);
        let mut bt = BehaviorTree::new_test(cond);

        let timer = sleep(Duration::from_millis(200));
        tokio::pin!(timer);
        tokio::select! {
            _ = &mut timer => {None}
            res = bt.run() => {Some(res)}
        };

        sleep(Duration::from_millis(200)).await;
        bt.kill().await;

        println!("Setting condition");
        handle.set(1).await;
        sleep(Duration::from_millis(200)).await;

        // TODO some assert that the tree is not reacting to the value of the condition being changed?
    }

    //  Cond1
    //    |
    // Action1
    // Pass cond1, throw error in Action, cond1 handles error while in stop
    #[tokio::test]
    async fn test_poison_while_stopping() {
        // Setup
        let handle = Handle::new(1);

        // When
        let action1 = MockAction::new_error(1);
        let cond1 = Condition::new("1", handle.clone(), |i: i32| i > 0, action1);

        let mut bt = BehaviorTree::new_test(cond1);

        let (res, _) = tokio::join!(bt.run_once(), async {
            sleep(Duration::from_millis(200)).await;
            // handle.set(-1).await // Condition asks action to stop
        });

        // Then
        assert!(matches!(res.unwrap_err(), NodeError::PoisonError(_)));
    }

    //      Fb
    //     /   \
    //   Seq  Action2
    //    |
    //  Cond1
    //    |
    // Action1
    //  Fail cond1, Start Action2, cond1 request start, seq success
    #[tokio::test]
    async fn test_sequence_request_start_while_failed() {
        // Setup
        let handle = Handle::new(-1);

        // When
        let action1 = MockAction::new(1);
        let cond1 = Condition::new("1", handle.clone(), |i: i32| i > 0, action1);
        let seq = Sequence::new(vec![cond1]);

        let action2 = MockAction::new_failing(2);
        let fb = Fallback::new(vec![seq, action2]);
        let mut bt = BehaviorTree::new_test(fb);

        let (res, _) = tokio::join!(bt.run_once(), async {
            sleep(Duration::from_millis(200)).await;
            handle.set(1).await
        });

        // Then
        assert_eq!(res.unwrap(), Status::Success);
    }

    //  Cond1
    //    |
    // Action1
    #[tokio::test]
    async fn test_auto_success() {
        // Setup
        let handle = Handle::new(1);

        // When
        let action1 = Success::new();
        let cond1 = Condition::new("1", handle.clone(), |x| x > 0, action1);
        let mut bt = BehaviorTree::new_test(cond1);

        // Then
        assert_eq!(bt.run_once().await.unwrap(), Status::Success);
    }

    //  Cond1
    //    |
    // Action1
    #[tokio::test]
    async fn test_root_restart_after_request() {
        // Setup
        let handle = Handle::new(-1);

        // When
        let action1 = Success::new();
        let cond1: NodeHandle = Condition::new("1", handle.clone(), |x| x > 0, action1);
        let mut bt = BehaviorTree::new_test(cond1);

        let (res, _) = tokio::join!(bt.start(), async {
            sleep(Duration::from_millis(200)).await;
            handle.set(1).await
        });

        // Then
        assert!(res.is_ok());
    }

    //      FB
    //    /    \
    //  Cond1  Action2
    //    |
    //  Cond2
    //    |
    // Action1
    // Cond1 succeeds, cond2 fails, cond1 gets updated but does not send request start, action2 fails
    #[tokio::test]
    async fn test_no_request_start_when_already_ok() {
        // Setup
        let handle1 = Handle::new(1);
        let handle2 = Handle::new(-1);

        // When
        let action1 = MockAction::new(1);
        let action2 = MockAction::fail_on_twice(2); // A request start will lead to action2 being restarted, as cond2 still fails.
        let cond2: NodeHandle = Condition::new("2", handle2.clone(), |x| x > 0, action1);
        let cond1: NodeHandle = Condition::new("1", handle1.clone(), |x| x > 0, cond2);
        let fb = Fallback::new(vec![cond1, action2]);
        let mut bt = BehaviorTree::new_test(fb);

        let (res, _) = tokio::join!(bt.run_once(), async {
            sleep(Duration::from_millis(200)).await;
            handle1.set(1).await // This value was already ok, but it should not lead to a request start, as it was already ok
        });

        // Then
        assert_eq!(res.unwrap(), Status::Success);
    }

    //  Cond1
    //    |
    // Action1
    #[tokio::test]
    async fn test_auto_failure() {
        // Setup
        let handle = Handle::new(1);

        // When
        let action1 = Failure::new();
        let cond1 = Condition::new("1", handle.clone(), |x| x > 0, action1);
        let mut bt = BehaviorTree::new_test(cond1);

        // Then
        assert_eq!(bt.run_once().await.unwrap(), Status::Failure);
    }

    //  Cond1
    //    |
    // Action1
    #[tokio::test]
    async fn test_listen_rx() {
        // Setup
        let handle = Handle::new(1);

        // When
        let action1 = MockAction::new(1);
        let cond1 = Condition::new("1", handle.clone(), |x| x > 0, action1);
        let mut bt = BehaviorTree::new_test(cond1);

        // Note that because the whole tree is run, it should not be allowed to repeat
        tokio::select! {
            err = bt.run() => {panic!("{err:?}");}
            _ = async {
                sleep(Duration::from_millis(1000)).await;
            } => {}
        };

        // Then
        let mut rx: Receiver<Update> = bt.take_rx().unwrap();
        let goal_statuses = vec![
            OuterStatus::Running,
            OuterStatus::Running,
            OuterStatus::Success,
            OuterStatus::Success,
        ];
        let mut received_statuses = vec![];
        while let Ok(update) = rx.try_recv() {
            received_statuses.push(update.status);
        }

        println!("received_statuses {:?}", received_statuses);

        for (index, status) in goal_statuses.iter().enumerate() {
            assert_eq!(status, &received_statuses[index])
        }
    }

    //  Cond1
    //    |
    // Action1
    //
    // Don't pas cond1
    #[tokio::test]
    async fn test_async_condition() {
        // Setup
        let handle: Handle<i32> = Handle::new(1);

        // When
        let action1 = MockAction::new(1);
        let cond1 = Condition::new_from(MockAsyncCondition::new(), handle, action1);
        let mut bt = BehaviorTree::new_test(cond1);

        // Then
        assert_eq!(bt.run_once().await.unwrap(), Status::Success);
    }

    //  Cond1
    //    |
    // Action1
    //
    // Don't pas cond1
    #[tokio::test]
    async fn test_if_vec_not_empty() {
        // Setup
        let handle: Handle<Vec<i32>> = Handle::new(vec![]);

        // When
        let action1 = MockAction::new(1);
        let cond1 = Condition::new("1", handle, |x| !x.is_empty(), action1);
        let mut bt = BehaviorTree::new_test(cond1);

        // Then
        assert_eq!(bt.run_once().await.unwrap(), Status::Failure);
    }

    //  Cond1
    //    |
    // Action1
    //
    // Don't pas cond1
    #[tokio::test]
    async fn test_if_map_not_empty() {
        // Setup
        let handle: Handle<HashMap<&str, i32>> = Handle::new(HashMap::new());

        // When
        let action1 = MockAction::new(1);
        let cond1 = Condition::new("1", handle, |x| !x.is_empty(), action1);
        let mut bt = BehaviorTree::new_test(cond1);

        // Then
        assert_eq!(bt.run_once().await.unwrap(), Status::Failure);
    }

    //      Seq
    //     /   \
    //  Cond1  Action2
    //    |
    // Action1
    //
    //  pass cond1, pass action1, pass action2, pass seq
    #[tokio::test]
    async fn test_simple_sequence() {
        // Setup
        let handle = Handle::new(1);

        // When
        let action1 = MockAction::new(1);
        let action2 = MockAction::new(2);
        let cond1 = Condition::new("1", handle, |i: i32| i > 0, action1);
        let seq = Sequence::new(vec![cond1, action2]);
        let mut bt = BehaviorTree::new_test(seq);

        // Then
        assert_eq!(bt.run_once().await.unwrap(), Status::Success);
    }

    //      FB
    //     /   \
    //  Cond1  Action2
    //    |
    // Action1
    //
    // pass cond1, pass action1, pass fb
    #[tokio::test]
    async fn test_simple_fallback_plan_a() {
        // Setup
        let handle = Handle::new(1);

        // When
        let action1 = MockAction::new(1);
        let action2 = MockAction::new(2);
        let cond1 = Condition::new("1", handle, |i: i32| i > 0, action1);
        let fb = Fallback::new(vec![cond1, action2]);
        let mut bt = BehaviorTree::new_test(fb);

        // Then
        assert_eq!(bt.run_once().await.unwrap(), Status::Success);
    }

    // Fail cond 1-3. Pass cond 4, then simultanously pass cond 1-3
    #[tokio::test]
    async fn test_fallback_multiple_requests() {
        // Setup
        let handle = Handle::new(-1);

        // When
        let action1 = MockAction::new(1);
        let action2 = Failure::new(); // Ensures that the test only succeeds if the first action completes
        let action3 = Failure::new();
        let action4 = MockAction::new_loop(4);
        let cond1 = Condition::new("1", handle.clone(), |i: i32| i > 0, action1);
        let cond2 = Condition::new("2", handle.clone(), |i: i32| i > 0, action2);
        let cond3 = Condition::new("3", handle.clone(), |i: i32| i > 0, action3);
        let cond4 = Condition::new("4", handle.clone(), |i: i32| i < 0, action4);
        let fb = Fallback::new(vec![cond1, cond2, cond3, cond4]);
        let mut bt = BehaviorTree::new_test(fb);

        let (res, _) = tokio::join!(bt.run_once(), async {
            sleep(Duration::from_millis(200)).await;
            handle.set(1).await
        });

        // Then
        assert_eq!(res.unwrap(), Status::Success);
    }

    //      Seq
    //     /   \
    //  Cond1  Action1
    //
    // pass cond1, during action1 fail cond1, still pass sequence
    #[tokio::test]
    async fn test_one_time_condition() {
        // Setup
        let handle = Handle::new(1);

        // When
        let action1 = MockAction::new(1);
        let cond1 = OneTimeCondition::new("1", handle.clone(), |i: i32| i > 0);
        let seq = Sequence::new(vec![cond1, action1]);
        let mut bt = BehaviorTree::new_test(seq);

        let (res, _) = tokio::join!(bt.run_once(), async {
            sleep(Duration::from_millis(200)).await;
            handle.set(-1).await
        });

        // Then
        assert_eq!(res.unwrap(), Status::Success);
    }

    //      FB
    //     /   \
    //  Cond1  Action2
    //    |
    // Action1
    //
    // Fail cond1, pass action2, pass fb
    #[tokio::test]
    async fn test_simple_fallback_plan_b() {
        // Setup
        let handle = Handle::new(-1);

        // When
        let action1 = MockAction::new(1);
        let action2 = MockAction::new(2);
        let cond1 = Condition::new("1", handle, |i: i32| i > 0, action1);
        let fb = Fallback::new(vec![cond1, action2]);
        let mut bt = BehaviorTree::new_test(fb);

        // Then
        assert_eq!(bt.run_once().await.unwrap(), Status::Success);
    }

    //      Seq
    //     /   \
    //  Cond1  Action2
    //    |
    //  Cond2
    //    |
    // Action1
    //
    // Pass Cond1, fail Cond2, Fail seq
    #[tokio::test]
    async fn test_double_condition_sequence() {
        // Setup
        let handle1 = Handle::new(1);
        let handle2: Handle<i32> = Handle::new(-1);

        // When
        let action1 = MockAction::new(1);
        let action2 = MockAction::new(2);
        let cond2 = Condition::new("2", handle2, |i: i32| i > 0, action1);
        let cond1 = Condition::new("1", handle1, |i: i32| i > 0, cond2);
        let seq = Sequence::new(vec![cond1, action2]);
        let mut bt = BehaviorTree::new_test(seq);

        // Then
        assert_eq!(bt.run_once().await.unwrap(), Status::Failure);
    }

    //      Seq
    //     /   \
    // Action1  Cond1
    //            |
    //         Action2
    //
    // Pass action 1, fail cond1 , fail seq
    #[tokio::test]
    async fn test_later_fail_of_sequence() {
        // Setup
        let handle = Handle::new(-1);

        // When
        let action1 = MockAction::new(1);
        let action2 = MockAction::new(2);
        let cond1 = Condition::new("1", handle.clone(), |i: i32| i > 0, action2);
        let seq = Sequence::new(vec![action1, cond1]);
        let mut bt = BehaviorTree::new_test(seq);

        // Then
        assert_eq!(bt.run_once().await.unwrap(), Status::Failure);
    }

    //      Seq
    //     /   \
    //  Cond1  Action2
    //    |
    // Action1
    //
    // Pass cond1, fail cond1 during action 1, fail seq
    #[tokio::test]
    async fn test_simple_sequence_with_subscribe() {
        // Setup
        let handle = Handle::new(1);

        // When
        let action1 = MockAction::new(1);
        let action2 = MockAction::new(2);
        let cond1 = Condition::new("1", handle.clone(), |i: i32| i > 0, action1);
        let seq = Sequence::new(vec![cond1, action2]);
        let mut bt = BehaviorTree::new_test(seq);

        let (res, _) = tokio::join!(bt.run_once(), async {
            sleep(Duration::from_millis(200)).await;
            handle.set(-1).await
        });

        // Then
        assert_eq!(res.unwrap(), Status::Failure);
    }

    //      FB
    //     /   \
    //  Cond1  Cond2
    //    |      |
    // Action1 Action2
    //
    // Fail cond1, Pass cond1 during action1, Fail cond2 during action2, pass fb
    #[tokio::test]
    async fn test_vec_not_empty_with_subscribe() {
        // Setup
        let handle1: Handle<Vec<i32>> = Handle::new(vec![]);
        let handle2 = Handle::new(1);

        // When
        let action1 = MockAction::new(1);
        let action2 = MockAction::new(2);
        let cond1 = Condition::new("1", handle1.clone(), |x| !x.is_empty(), action1);
        let cond2 = Condition::new("2", handle2.clone(), |i: i32| i > 0, action2);
        let fb = Fallback::new(vec![cond1, cond2]);
        let mut bt = BehaviorTree::new_test(fb);

        let (res, _, _) = tokio::join!(
            bt.run_once(),
            async {
                sleep(Duration::from_millis(200)).await;
                handle1.set(vec![i32::default()]).await
            },
            async {
                sleep(Duration::from_millis(400)).await;
                handle2.set(-1).await
            }
        );

        // Then
        assert_eq!(res.unwrap(), Status::Success);
    }

    //      FB
    //     /   \
    //  Cond1  Action2
    //    |
    // Action1
    //
    // Fail cond1, pass cond1 during action 2, switch back to action 1, pass fb
    #[tokio::test]
    async fn test_fallback_switch_to_prio() {
        // Setup
        let handle = Handle::new(-1);

        // When
        let action1 = MockAction::new(1);
        let action2 = MockAction::new(2);
        let cond1 = Condition::new("1", handle.clone(), |i: i32| i > 0, action1);
        let fb = Fallback::new(vec![cond1, action2]);
        let mut bt = BehaviorTree::new_test(fb);

        let (res, _) = tokio::join!(bt.run_once(), async {
            sleep(Duration::from_millis(200)).await;
            handle.set(1).await
        });

        // Then
        assert_eq!(res.unwrap(), Status::Success);
    }

    //      Seq
    //     /   \
    //  Cond1   Action2
    //    |
    //  Cond2
    //    |
    // Action1
    //
    // pass cond1, pass cond2, fail cond2 during action 1, fail cond1, fail seq
    #[tokio::test]
    async fn test_double_condition_sequence_with_subscribe() {
        // Setup
        let handle1 = Handle::new(1);
        let handle2 = Handle::new(1);

        // When
        let action1 = MockAction::new(1);
        let action2 = MockAction::new(2);
        let cond2 = Condition::new("2", handle2.clone(), |i: i32| i > 0, action1);
        let cond1 = Condition::new("1", handle1, |i: i32| i > 0, cond2);
        let seq = Sequence::new(vec![cond1, action2]);
        let mut bt = BehaviorTree::new_test(seq);

        let (res, _) = tokio::join!(bt.run_once(), async {
            sleep(Duration::from_millis(200)).await;
            handle2.set(-1).await
        });

        // Then
        assert_eq!(res.unwrap(), Status::Failure);
    }

    //     Cond1
    //       |
    //      Seq
    //     /   \
    // Action1 Action2
    //
    // pass cond1, fail cond1 during action 1, fail seq
    #[tokio::test]
    async fn test_conditional_sequence_with_subscribe() {
        // Setup
        let handle = Handle::new(1);

        // When
        let action1 = MockAction::new(1);
        let action2 = MockAction::new(2);
        let seq = Sequence::new(vec![action1, action2]);
        let cond1 = Condition::new("1", handle.clone(), |i: i32| i > 0, seq);
        let mut bt = BehaviorTree::new_test(cond1);

        // let cond2 fail during execution
        let (res, _) = tokio::join!(bt.run_once(), async {
            sleep(Duration::from_millis(200)).await;
            handle.set(-1).await
        });

        // Then
        assert_eq!(res.unwrap(), Status::Failure);
    }

    //          FB
    //        /   \
    //     Cond1  Action3
    //       |
    //       FB
    //     /   \
    //  Cond2  Action2
    //    |
    // Action1
    //
    // Pass cond1, fail cond2, fail cond1 during action2, pass cond2 during action3 (no effect), pass fb
    #[tokio::test]
    async fn test_failed_fallback_with_delayed_child_request() {
        // Setup
        let handle1 = Handle::new(1);
        let handle2 = Handle::new(-1);

        // When
        let action1 = MockAction::new(1);
        let cond2 = Condition::new("2", handle2.clone(), |i: i32| i > 0, action1);
        let action2 = MockAction::new(2);
        let fb2 = Fallback::new(vec![cond2, action2]);
        let cond1 = Condition::new("1", handle1.clone(), |i: i32| i > 0, fb2);
        let action3 = MockAction::new(3);
        let fb1 = Fallback::new(vec![cond1, action3]);
        let mut bt = BehaviorTree::new_test(fb1);

        let (res, _, _) = tokio::join!(
            bt.run_once(),
            async {
                sleep(Duration::from_millis(200)).await;
                handle1.set(-1).await
            },
            async {
                sleep(Duration::from_millis(400)).await;
                handle2.set(1).await
            }
        );

        // Then
        assert_eq!(res.unwrap(), Status::Success);
    }

    //     Cond1
    //       |
    //       FB
    //     /   \
    //  Cond2  Action2
    //    |
    // Action1
    //
    // Pass cond1, pass cond2, fail cond1 during action1
    #[tokio::test]
    async fn test_prohibited_fallback() {
        // Setup
        let handle1 = Handle::new(1);
        let handle2 = Handle::new(1);

        // When
        let action1 = MockAction::new(1);
        let cond2 = Condition::new("2", handle2.clone(), |i: i32| i > 0, action1);
        let action2 = MockAction::new(2);
        let fb = Fallback::new(vec![cond2, action2]);
        let cond1 = Condition::new("1", handle1.clone(), |i: i32| i > 0, fb);
        let mut bt = BehaviorTree::new_test(cond1);

        let (res, _) = tokio::join!(bt.run_once(), async {
            sleep(Duration::from_millis(200)).await;
            handle1.set(-1).await
        },);

        // Then
        assert_eq!(res.unwrap(), Status::Failure);
    }

    //       FB
    //     /   \
    //  Cond1   Cond3
    //    |      |
    //  Cond2  Action2
    //    |
    // Action1
    //
    // pass cond1, fail cond2, pass cond3, pass cond2 during action 2, fail cond3 during action 1 (no effect), pass fb
    #[tokio::test]
    async fn test_double_request_start_before_failing_fallback() {
        // Setup
        let handle1 = Handle::new(1);
        let handle2 = Handle::new(-1);
        let handle3 = Handle::new(1);

        // When
        let action1 = MockAction::new(1);
        let cond2 = Condition::new("2", handle2.clone(), |i: i32| i > 0, action1);
        let cond1 = Condition::new("1", handle1, |i: i32| i > 0, cond2);
        let action2 = MockAction::new(2);
        let cond3 = Condition::new("3", handle3.clone(), |i: i32| i > 0, action2);
        let fb = Fallback::new(vec![cond1, cond3]);
        let mut bt = BehaviorTree::new_test(fb);

        let (res, _, _) = tokio::join!(
            bt.run_once(),
            async {
                sleep(Duration::from_millis(200)).await;
                handle2.set(1).await
            },
            async {
                sleep(Duration::from_millis(400)).await;
                handle3.set(-1).await
            }
        );

        // Then
        assert_eq!(res.unwrap(), Status::Success);
    }

    // Redundant with tests below
    #[tokio::test]
    async fn test_true_if_success() {
        let action = MockAction::new(1);
        let mut bt = BehaviorTree::new_test(action);
        
        // Then
        assert_eq!(bt.execute().await.unwrap(), true);
    }

    #[tokio::test]
    async fn test_false_if_failure() {
        let action = MockAction::new_failing(1);
        let mut bt = BehaviorTree::new_test(action);
        
        // Then
        assert_eq!(bt.execute().await.unwrap(), false);
    }

    #[tokio::test]
    async fn test_no_return_if_running() {
        let action = MockAction::new_loop(1);
        let mut bt = BehaviorTree::new_test(action);

        let res = tokio::time::timeout(std::time::Duration::from_secs(1), bt.execute()).await;
        assert!(res.is_err(), "bt.run() unexpectedly returned: {:?}", res);
    }

    #[tokio::test]
    async fn test_all_nodes_killed_after_return() {
        let action = MockAction::new(123456);
        let mut bt = BehaviorTree::new_test(action);

        let logger = Logger::start();
        assert_eq!(bt.execute().await.unwrap(), true);

        let logs: Vec<_> = logger.collect();

        // filter all logs mentioning node 123456
        let node_logs: Vec<_> = logs
            .iter()
            .filter(|rec| rec.args().contains("123456")) // adjust to match how the repo formats
            .collect();

        assert!(
            !node_logs.is_empty(),
            "No logs found for action. Logs: {:?}",
            logs
        );

        // check the last log line for this node contains "Idle"
        let last = node_logs.last().unwrap();
        assert!(
            last.args().contains("Killed"),
            "Expected final state Killed for Action, got: {:?}",
            node_logs
        );
    }

    //      Seq
    //     /   \
    //   Seq  Action2
    //    |  
    //   FB
    //    |  
    //  Cond1
    //    |  
    // Action1 
    // pass cond1, pass action1, cond1 fails during action2, seq fails
    // finished selectors do not block the switch
    #[tokio::test]
    async fn test_sequence_failing_succeeded_condition() {
        // Setup
        let handle1 = Handle::new(1);

        // When
        let action1 = MockAction::new(1);
        let cond1 = Condition::new("1", handle1.clone(), |i: i32| i > 0, action1);
        let action2 = Wait::new(Duration::from_millis(200));
        let subfb = Fallback::new(vec![cond1]);
        let subsq = Sequence::new(vec![subfb]);
        let sq = Sequence::new(vec![subsq, action2]);
        let mut bt = BehaviorTree::new_test(sq);

        let (res, _) = tokio::join!(
            bt.execute(),
            async {
                sleep(Duration::from_millis(600)).await;
                handle1.set(-1).await;
            }
        );

        // Then
        assert_eq!(res.unwrap(), false);
    }

    //       FB
    //     /   \
    //   Seq  Action2
    //    |  
    //   FB
    //    |  
    //  Cond1
    //    |  
    // Action1    
    // fail cond1, cond1 passes during action2, action1 passes, FB passes
    #[tokio::test]
    async fn test_fallback_passing_failed_condition() {
        // Setup
        let handle1 = Handle::new(-1);

        // When
        let action1 = MockAction::new(1);
        let cond1 = Condition::new("1", handle1.clone(), |i: i32| i > 0, action1);
        let action2 = MockAction::new_failing(2);
        let subfb = Fallback::new(vec![cond1]);
        let sq = Sequence::new(vec![subfb]);
        let fb = Fallback::new(vec![sq, action2]);
        let mut bt = BehaviorTree::new_test(fb);

        let (res, _) = tokio::join!(
            bt.execute(),
            async {
                sleep(Duration::from_millis(400)).await;
                handle1.set(1).await;
            }
        );

        // Then
        assert_eq!(res.unwrap(), true);
    }
    // Tests to show unexpected behavior

    //         Seq
    //        /   \
    //      FB   Action
    //    /   \     
    //  Cond  Mit. Act.
    //    |
    // Success
    //
    // Expected behavior: when Condition succeeds, execute Action. If condition switches to failure, take Mitigation Action, it fails, failing the tree
    // Actual behavior: Action is not interrupted, succeeds, and thus succeeding the tree
    // Cause: Conditions that have succeeded never request to be started/failed again
    #[tokio::test]
    async fn test_safety_pattern() {
        // Setup
        let handle1 = Handle::new(1);

        // When
        let action = Wait::new(Duration::from_secs(1)); // Long-lasting main action
        let success = Success::new();
        let cond = Condition::new("1", handle1.clone(), |i: i32| i > 0, success);
        let mitigation_action = MockAction::new_failing(1); // Failing mitigation
        let fb = Fallback::new(vec![cond, mitigation_action]);
        let seq = Sequence::new(vec![fb, action]);
        let mut bt = BehaviorTree::new_test(seq);

        let (res, _) = tokio::join!(
            bt.execute(),
            async {
                sleep(Duration::from_millis(400)).await;
                handle1.set(-1).await
            }
        );

        // Then 
        assert_eq!(res.unwrap(), false);
    }

    //          FB
    //         /   \
    //       Seq   Action
    //      /   \     
    // not Cond  Mit. Act.
    //    |
    // Success
    //
    // Switch everything around: Condition that have failed, do switch. Try it by inverting the condition and selectors. Mitigation Action result also reversed
    // Expected behavior: when Condition succeeds, execute Action. If condition switches to failure, take Mitigation Action, it succeeds, succeeding the tree
    #[tokio::test]
    async fn test_safety_pattern_flipped() {
        // Setup
        let handle1 = Handle::new(1);

        // When
        let action = MockAction::new_failing(1); // Long-lasting main action
        let success = Success::new();
        let cond = Condition::new("1", handle1.clone(), |i: i32| i < 0, success);
        let mitigation_action = MockAction::new(2); // Succeeding mitigation
        let seq = Sequence::new(vec![cond, mitigation_action]);
        let fb = Fallback::new(vec![seq, action]);
        let mut bt = BehaviorTree::new_test(fb);

        let (res, _) = tokio::join!(
            bt.execute(),
            async {
                sleep(Duration::from_millis(200)).await;
                handle1.set(-1).await
            }
        );

        // Then 
        assert_eq!(res.unwrap(), true);
    }

    //          FB
    //         /   \
    //       Seq   Action
    //      /   \     
    // not Cond  Mit. Act.
    //    |
    // Success
    //
    // This seems to work, but:
    // Same flow, but the Mitigation fails. Action is then again executed. Say the condition switches from success to failure to success, so the mitigation should start a second time
    // Expected behavior: Same flow, Mitigation fails, Action starts again, condition interupts action and mitigation fails again. Action runs and tree fails
    // Actual behavior: the action runs uninterrupted
    #[tokio::test]
    async fn test_safety_pattern_flipped_two_mitigations() {
        // Setup
        let handle1 = Handle::new(1);

        // When
        let action = MockAction::new_failing(1); // Long-lasting main action
        let success = Success::new();
        let cond = Condition::new("1", handle1.clone(), |i: i32| i < 0, success);
        let mitigation_action = MockAction::new_failing(2); // Succeeding mitigation
        let seq = Sequence::new(vec![cond, mitigation_action]);
        let fb = Fallback::new(vec![seq, action]);
        let mut bt = BehaviorTree::new_test(fb);

        let (res, _, _, _) = tokio::join!(
            bt.execute(),
            async {
                sleep(Duration::from_millis(200)).await;
                handle1.set(-1).await
            },
            async {
                sleep(Duration::from_millis(450)).await;
                handle1.set(1).await
            },
            async {
                sleep(Duration::from_millis(500)).await;
                handle1.set(-1).await
            }
        );

        // Then (expected outcome = real outcome, but behavior is wrong!)
        assert_eq!(res.unwrap(), false);
    }
}