use std::time::Duration;

use fake::{
    faker::{internet::en::SafeEmail, name::en::Name},
    Fake,
};
use wiremock::{
    matchers::{any, method, path},
    Mock, MockBuilder, ResponseTemplate,
};

use crate::helper::{assert_is_redirect_to, spawn_app, ConfirmationLinks, TestApp};

#[tokio::test]
async fn newsletters_are_not_delivered_to_unconfirmed_subscribers() {
    let app = spawn_app().await;
    app.post_login(&serde_json::json!({
        "username": &app.test_user.username,
        "password": &app.test_user.password,
    }))
    .await;
    create_unconfirmed_subscriber(&app).await;
    // To Assert No req fired to email service
    Mock::given(any())
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&app.email_server)
        .await;

    let newsletter_req_body = serde_json::json!({
        "title": "Newsletter Title",
        "text_content": "Newsletter in plain text",
        "html_content": "<h1>Newsletter</h1> as HTML",
        "idempotency_key": uuid::Uuid::new_v4().to_string()
    });

    let response = app.post_newsletter(&newsletter_req_body).await;
    assert_is_redirect_to(&response, "/admin/newsletters");
    app.dispatch_all_pending_emails().await;
}

#[tokio::test]
async fn newsletters_are_delivered_to_confirmed_subscriber() {
    let app = spawn_app().await;
    app.post_login(&serde_json::json!({
        "username": &app.test_user.username,
        "password": &app.test_user.password,
    }))
    .await;
    create_confirmed_subscriber(&app).await;

    when_sending_an_email()
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&app.email_server)
        .await;
    let newsletter_req_body = serde_json::json!({
        "title": "Newsletter Title",
        "text_content": "Newsletter in plain text",
        "html_content": "<h1>Newsletter</h1> as HTML",
        "idempotency_key": uuid::Uuid::new_v4().to_string()
    });

    let response = app.post_newsletter(&newsletter_req_body).await;

    assert_is_redirect_to(&response, "/admin/newsletters");
    app.dispatch_all_pending_emails().await;
}

#[tokio::test]
async fn newsletters_returns_400_for_invalid_body() {
    let app = spawn_app().await;
    app.post_login(&serde_json::json!({
        "username": &app.test_user.username,
        "password": &app.test_user.password,
    }))
    .await;
    let test_case = [
        (
            serde_json::json!({
                "text_content": "newsletter plain text",
                "html_content": "<h1>newsletter</h1> html"
            }),
            "missing title",
        ),
        (
            serde_json::json!({
                "title": "test title"
            }),
            "missing content",
        ),
    ];

    for (invalid_body, msg) in test_case {
        let res = app.post_newsletter(&invalid_body).await;

        assert_eq!(
            res.status().as_u16(),
            400,
            "The API did not fail with status 400 when payload was {msg}"
        );
    }
}

#[tokio::test]
async fn newsletter_creation_is_idempotent() {
    let app = spawn_app().await;
    create_confirmed_subscriber(&app).await;
    app.test_user.login(&app).await;

    when_sending_an_email()
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&app.email_server)
        .await;
    let newsletter_req_body = serde_json::json!({
        "title": "Newsletter title",
        "html_content": "<h1>Newsletter  body</h1>",
        "text_content": "Newsletter body",
        "idempotency_key": uuid::Uuid::new_v4().to_string()
    });
    // submit newsletter form
    let response = app.post_newsletter(&newsletter_req_body).await;
    assert_is_redirect_to(&response, "/admin/newsletters");

    // follow request
    let html_page = app.post_newsletter_html().await;
    assert!(
        html_page.contains("The newsletter issue has been accepted - emails will go out shortly.")
    );

    // submit newsletter again
    let response = app.post_newsletter(&newsletter_req_body).await;
    assert_is_redirect_to(&response, "/admin/newsletters");

    let html_page = app.post_newsletter_html().await;
    assert!(
        html_page.contains("The newsletter issue has been accepted - emails will go out shortly.")
    );
    app.dispatch_all_pending_emails().await;
}

#[tokio::test]
async fn concurrent_form_submission_is_handled() {
    let app = spawn_app().await;
    create_confirmed_subscriber(&app).await;
    app.test_user.login(&app).await;

    when_sending_an_email()
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(2)))
        .expect(1)
        .mount(&app.email_server)
        .await;
    let newsletter_req_body = serde_json::json!({
        "title": "Newsletter title",
        "html_content": "<h1>Newsletter  body</h1>",
        "text_content": "Newsletter body",
        "idempotency_key": uuid::Uuid::new_v4().to_string()
    });

    // submi two requests
    let response1 = app.post_newsletter(&newsletter_req_body);
    let response2 = app.post_newsletter(&newsletter_req_body);
    let (response1, response2) = tokio::join!(response1, response2);

    assert_eq!(response1.status(), response2.status());
    assert_eq!(
        response1.text().await.unwrap(),
        response2.text().await.unwrap()
    );
    app.dispatch_all_pending_emails().await;
}

fn when_sending_an_email() -> MockBuilder {
    Mock::given(path("/email")).and(method("POST"))
}

async fn create_unconfirmed_subscriber(app: &TestApp) -> ConfirmationLinks {
    let name: String = Name().fake();
    let email: String = SafeEmail().fake();
    let body = serde_urlencoded::to_string(serde_json::json!({
        "name": name,
        "email": email,
    }))
    .unwrap();
    let _mock_guard = Mock::given(path("/email"))
        .and(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .named("Create unconfirmed subscriber")
        .expect(1)
        .mount_as_scoped(&app.email_server)
        .await;
    app.post_subscriptions(body)
        .await
        .error_for_status()
        .unwrap();
    let email_req = &app.email_server.received_requests().await.unwrap()[0];
    app.get_confirmation_link(email_req)
}

async fn create_confirmed_subscriber(app: &TestApp) {
    let confirmation_link = create_unconfirmed_subscriber(app).await;
    reqwest::get(confirmation_link.html)
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
}
