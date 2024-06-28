use actix_web::{http::header::ContentType, web, HttpResponse};
use hmac::{Hmac, Mac};
use secrecy::ExposeSecret;

use crate::startup::HmacSecret;

#[derive(serde::Deserialize)]
pub struct QueryParam {
    error: String,
    tag: String,
}

impl QueryParam {
    fn verify(self, secret: &HmacSecret) -> Result<String, anyhow::Error> {
        let tag = hex::decode(self.tag)?;
        let query_string = format!("error={}", urlencoding::encode(&self.error));

        let mut mac =
            Hmac::<sha2::Sha256>::new_from_slice(secret.0.expose_secret().as_bytes()).unwrap();
        mac.update(query_string.as_bytes());
        mac.verify_slice(&tag)?;
        Ok(self.error)
    }
}

pub async fn login_form(
    secret: web::Data<HmacSecret>,
    query: Option<web::Query<QueryParam>>,
) -> HttpResponse {
    let err_html = match query {
        Some(query) => match query.0.verify(&secret) {
            Ok(error_msg) => {
                format!("<p><i>{}</i></p>", htmlescape::encode_minimal(&error_msg))
            }
            Err(e) => {
                tracing::warn!(
                error.message = %e,
                error.cause_chain = ?e,
                "Failed to verify query parameters using the HMAC tag"
                );
                "".into()
            }
        },
        None => "".into(),
    };

    HttpResponse::Ok()
        .content_type(ContentType::html())
        .body(format!(
            r#"
<!DOCTYPE html>
<html lang="en">

<head>
    <meta http-equiv="content-type" content="text/html; charset=utf-8">
    <title>Login</title>
</head>

<body style="display: flex; align-items: center; justify-content: center; flex-direction: column; min-height:100vh">
{err_html}
<form action="/login" method="post" style="padding: 10px; border: 1px solid blueviolet;">
        <label>Username
            <input type="text" placeholder="Enter Username" name="username">
        </label>
        <label>Password
            <input type="password" placeholder="Enter Password" name="password">
        </label>
        <button type="submit">Login</button>
    </form>
</body>

</html>
            "#
        ))
}
