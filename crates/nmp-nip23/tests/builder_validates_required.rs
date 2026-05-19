//! `ArticleBuilder` rejects an empty `d` tag with `ArticleBuildError::MissingDTag`.

use nmp_nip23::{Article, ArticleBuildError};

#[test]
fn empty_d_tag_returns_err_not_panic() {
    let result = Article::new("").build("author", 0);
    assert!(matches!(result, Err(ArticleBuildError::MissingDTag)));
}

#[test]
fn whitespace_only_d_tag_returns_err() {
    // A `d` tag that's only whitespace is semantically empty — the store
    // would still index it and silently collapse all articles by this author.
    let result = Article::new("\t  \n").build("author", 0);
    assert!(matches!(result, Err(ArticleBuildError::MissingDTag)));
}

#[test]
fn valid_d_tag_with_no_other_fields_succeeds() {
    Article::new("intro").build("author", 0).expect("d_tag alone is valid");
}
