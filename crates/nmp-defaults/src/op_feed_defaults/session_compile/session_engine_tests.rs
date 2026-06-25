use super::session_engine::{acquisition_children, AcquisitionInterest, ExtraAcquisition};
use nmp_core::DependentInterestChild;
use nmp_planner::InterestShape;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

fn author_shape(author: &str) -> InterestShape {
    InterestShape::timeline_for(
        [author.to_string()].into_iter().collect(),
        [1_u32].into_iter().collect(),
    )
}

fn authors(children: &[DependentInterestChild]) -> BTreeSet<String> {
    children
        .iter()
        .flat_map(|child| child.interest.shape.authors.iter().cloned())
        .collect()
}

#[test]
fn acquisition_children_reflect_current_extra_snapshot() {
    let fixed = vec![AcquisitionInterest::global(author_shape("fixed"))];
    let live = Arc::new(Mutex::new(vec![
        AcquisitionInterest::active_account(author_shape("a")),
        AcquisitionInterest::active_account(author_shape("b")),
    ]));
    let extra: ExtraAcquisition = {
        let live = Arc::clone(&live);
        Arc::new(move || live.lock().expect("extra snapshot").clone())
    };

    let first = acquisition_children(&fixed, &extra);
    assert_eq!(
        authors(&first),
        BTreeSet::from(["a".to_string(), "b".to_string(), "fixed".to_string()])
    );

    *live.lock().expect("extra snapshot") =
        vec![AcquisitionInterest::active_account(author_shape("a"))];
    let shrunk = acquisition_children(&fixed, &extra);
    assert_eq!(
        authors(&shrunk),
        BTreeSet::from(["a".to_string(), "fixed".to_string()])
    );
}
