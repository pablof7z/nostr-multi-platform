use super::*;

const TOPIC: &str = "bitcoin";
const CONSUMER: &str = "discover-view";

fn run_execute(action: TopicArticlesAction) -> Vec<ActorCommand> {
    let cmds = std::cell::RefCell::new(Vec::new());
    TopicArticlesModule
        .execute(action, "test-cid", &|cmd| cmds.borrow_mut().push(cmd))
        .expect("execute must not fail for valid input");
    cmds.into_inner()
}

#[test]
fn claim_sends_direct_and_repost_interests_for_topic_feed() {
    let cmds = run_execute(TopicArticlesAction::Claim {
        topic: TOPIC.to_string(),
        consumer_id: CONSUMER.to_string(),
    });
    assert_eq!(cmds.len(), 2);

    let ActorCommand::Interests(InterestsCommand::EnsureInterest {
        identity: direct_identity,
        interest: direct_interest,
    }) = &cmds[0]
    else {
        panic!("expected EnsureInterest, got {:?}", cmds[0]);
    };
    assert_eq!(*direct_identity, topic_articles_identity(TOPIC, CONSUMER));
    assert_eq!(direct_interest.id, topic_articles_interest_id(TOPIC));
    assert_eq!(
        direct_interest.shape.kinds,
        [KIND_LONG_FORM_ARTICLE].into_iter().collect()
    );
    assert_eq!(
        direct_interest
            .shape
            .tags
            .get("t")
            .and_then(|v| v.iter().next().map(|s| s.as_str())),
        Some(TOPIC)
    );
    assert!(direct_interest.is_indexer_discovery);

    let ActorCommand::Interests(InterestsCommand::EnsureInterest {
        identity: repost_identity,
        interest: repost_interest,
    }) = &cmds[1]
    else {
        panic!("expected repost EnsureInterest, got {:?}", cmds[1]);
    };
    assert_eq!(
        *repost_identity,
        topic_article_reposts_identity(TOPIC, CONSUMER)
    );
    assert_eq!(repost_interest.id, topic_article_reposts_interest_id(TOPIC));
    assert_eq!(
        repost_interest.shape.kinds,
        [nmp_nip18::KIND_GENERIC_REPOST].into_iter().collect()
    );
    assert_eq!(
        repost_interest
            .shape
            .tags
            .get("k")
            .and_then(|v| v.iter().next().map(|s| s.as_str())),
        Some("30023")
    );
    assert!(
        !repost_interest.shape.tags.contains_key("t"),
        "kind:16 wrappers cannot be relay-filtered by target article topic"
    );
    assert!(repost_interest.is_indexer_discovery);
}

#[test]
fn release_drops_direct_and_repost_interest_owners() {
    let cmds = run_execute(TopicArticlesAction::Release {
        topic: TOPIC.to_string(),
        consumer_id: CONSUMER.to_string(),
    });
    assert_eq!(cmds.len(), 2);
    let ActorCommand::Interests(InterestsCommand::DropInterestOwner(direct_identity)) = &cmds[0] else {
        panic!("expected DropInterestOwner, got {:?}", cmds[0]);
    };
    assert_eq!(*direct_identity, topic_articles_identity(TOPIC, CONSUMER));
    let ActorCommand::Interests(InterestsCommand::DropInterestOwner(repost_identity)) = &cmds[1] else {
        panic!("expected DropInterestOwner, got {:?}", cmds[1]);
    };
    assert_eq!(
        *repost_identity,
        topic_article_reposts_identity(TOPIC, CONSUMER)
    );
}

#[test]
fn claim_and_release_derive_identical_identity() {
    assert_eq!(
        topic_articles_identity(TOPIC, CONSUMER),
        topic_articles_identity(TOPIC, CONSUMER)
    );
    assert_eq!(
        topic_article_reposts_identity(TOPIC, CONSUMER),
        topic_article_reposts_identity(TOPIC, CONSUMER)
    );
}

#[test]
fn different_consumers_same_topic_have_distinct_owner_keys() {
    let a = topic_articles_identity(TOPIC, "view-a");
    let b = topic_articles_identity(TOPIC, "view-b");
    assert_ne!(a.owner, b.owner);
    assert_eq!(a.key, b.key);
    assert_eq!(a.scope, b.scope);

    let repost_a = topic_article_reposts_identity(TOPIC, "view-a");
    let repost_b = topic_article_reposts_identity(TOPIC, "view-b");
    assert_ne!(repost_a.owner, repost_b.owner);
    assert_eq!(repost_a.key, repost_b.key);
    assert_ne!(a.key, repost_a.key);
}

#[test]
fn different_topics_have_distinct_slot_keys() {
    assert_ne!(
        topic_articles_identity("bitcoin", CONSUMER).key,
        topic_articles_identity("zaps", CONSUMER).key
    );
    assert_ne!(
        topic_article_reposts_identity("bitcoin", CONSUMER).key,
        topic_article_reposts_identity("zaps", CONSUMER).key
    );
}

#[test]
fn start_rejects_empty_topic() {
    let mut ctx = ActionContext::default();
    let action = TopicArticlesAction::Claim {
        topic: String::new(),
        consumer_id: CONSUMER.to_string(),
    };
    assert!(matches!(
        TopicArticlesModule.start(&mut ctx, action),
        Err(ActionRejection::Invalid(_))
    ));
}

#[test]
fn start_rejects_empty_consumer_id() {
    let mut ctx = ActionContext::default();
    let action = TopicArticlesAction::Claim {
        topic: TOPIC.to_string(),
        consumer_id: String::new(),
    };
    assert!(matches!(
        TopicArticlesModule.start(&mut ctx, action),
        Err(ActionRejection::Invalid(_))
    ));
}

#[test]
fn interest_id_is_stable_across_calls() {
    assert_eq!(
        topic_articles_interest_id(TOPIC),
        topic_articles_interest_id(TOPIC)
    );
    assert_ne!(
        topic_articles_interest_id("bitcoin"),
        topic_articles_interest_id("nostr")
    );
    assert_eq!(
        topic_article_reposts_interest_id(TOPIC),
        topic_article_reposts_interest_id(TOPIC)
    );
    assert_ne!(
        topic_articles_interest_id(TOPIC),
        topic_article_reposts_interest_id(TOPIC)
    );
}
