//! Authentication middleware for Actix Web.
//!
//! `auth-middleware-for-actix-web` makes it easy to configure authentication in Actix Web.
//! It provides a middleware with which secured paths can be defined globally and it provides an extractor [AuthToken] that can be used, to
//! retrieve the currently logged in user.
//!
//! # Examples
//! ## For session based authentication ([Actix Session](https://docs.rs/actix-session/latest/actix_session/)).
//! ```no_run
//! use actix_session::{storage::CookieSessionStore, SessionMiddleware};
//! use actix_web::{cookie::Key, App, HttpServer};
//! use auth_middleware_for_actix_web::{middleware::{AuthMiddleware, PathMatcher}, session::session_auth::{SessionAuthProvider}};
//! use serde::{Deserialize, Serialize};
//!
//! fn create_actix_session_middleware() -> SessionMiddleware<CookieSessionStore> {
//!     let key = Key::generate();
//!    
//!     SessionMiddleware::new(CookieSessionStore::default(), key.clone())
//! }
//! #[actix_web::main]
//! async fn main() -> std::io::Result<()> {
//!     HttpServer::new(move || {
//!         App::new()
//!           .wrap(AuthMiddleware::<_, User>::new(SessionAuthProvider, PathMatcher::default()))
//!             .wrap(create_actix_session_middleware())
//!     })
//!     .bind(("127.0.0.1", 8080))?
//!     .run()
//!     .await
//! }
//!
//! #[derive(Serialize, Deserialize)]
//! pub struct User {
//!    pub email: String,
//!    pub name: String,
//! }
//! ```

use actix_web::{Error, FromRequest, HttpMessage, HttpRequest, HttpResponse, ResponseError};
use core::fmt;
use serde::de::DeserializeOwned;
use std::{
    cell::{Ref, RefCell},
    future::{ready, Future, Ready},
    pin::Pin,
    rc::Rc,
};

pub mod middleware;
pub mod session;

/// This trait is used to retrieve the logged in user.
/// If no user was found (e.g. in Actix-Session) it will return an Err.
///
/// Currently it is only implemented for actix-session:
/// [SessionAuthProvider](crate::session::session_auth::SessionAuthProvider)
pub trait AuthenticationProvider<U>
where
    U: DeserializeOwned + 'static,
{
    fn get_authenticated_user(
        &self,
        req: &HttpRequest,
    ) -> Pin<Box<dyn Future<Output = Result<U, UnauthorizedError>>>>;
    fn invalidate(&self, req: HttpRequest) -> Pin<Box<dyn Future<Output = ()>>>;
}

/// Extractor that holds the authenticated user
///
/// [`AuthToken`] will be used to handle the logged in user within secured routes. If you inject it a route that is not secured,
/// an [UnauthorizedError] is thrown.
/// Retrieve the current user:
/// ```ignore
/// #[get("/secured-route")]
/// pub async fn secured_route(token: AuthToken<User>) -> impl Responder {
///     HttpResponse::Ok().body(format!(
///         "Request from user: {}",
///         token.get_authenticated_user().email
///     ))
/// }
/// ```
/// You can also initiate a logout with:
/// ```ignore
/// #[post("/logout")]
/// pub async fn logout(token: AuthToken<User>) -> impl Responder {
///     token.invalidate();
///     HttpResponse::Ok()
/// }
/// ```
pub struct AuthToken<U>
where
    U: DeserializeOwned,
{
    inner: Rc<RefCell<AuthTokenInner<U>>>,
}

impl<U> AuthToken<U>
where
    U: DeserializeOwned,
{
    pub fn get_authenticated_user(&self) -> Ref<U> {
        Ref::map(self.inner.borrow(), |inner| &inner.user)
    }

    pub(crate) fn is_valid(&self) -> bool {
        let inner = self.inner.borrow();
        inner.is_valid
    }

    pub fn invalidate(&self) {
        let mut inner = self.inner.as_ref().borrow_mut();
        inner.is_valid = false;
    }

    pub(crate) fn new(user: U) -> Self {
        Self {
            inner: Rc::new(RefCell::new(AuthTokenInner {
                user,
                is_valid: true,
            })),
        }
    }

    pub(crate) fn from_ref(token: &AuthToken<U>) -> Self {
        AuthToken {
            inner: Rc::clone(&token.inner),
        }
    }
}

struct AuthTokenInner<U>
where
    U: DeserializeOwned,
{
    user: U,
    is_valid: bool,
}

impl<U> FromRequest for AuthToken<U>
where
    U: DeserializeOwned + 'static,
{
    type Error = Error;
    type Future = Ready<Result<AuthToken<U>, Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut actix_web::dev::Payload) -> Self::Future {
        let extensions = req.extensions();
        if let Some(token) = extensions.get::<AuthToken<U>>() {
            return ready(Ok(AuthToken::from_ref(token)));
        }

        ready(Err(UnauthorizedError::default().into()))
    }
}

#[derive(Debug)]
pub struct UnauthorizedError {
    message: String,
}

impl UnauthorizedError {
    pub fn new(message: &str) -> Self {
        Self {
            message: message.to_owned(),
        }
    }
}

impl Default for UnauthorizedError {
    fn default() -> Self {
        Self {
            message: "Not authorized".to_owned(),
        }
    }
}

impl fmt::Display for UnauthorizedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Debug unauth error")
    }
}

impl ResponseError for UnauthorizedError {
    fn status_code(&self) -> actix_web::http::StatusCode {
        actix_web::http::StatusCode::UNAUTHORIZED
    }

    fn error_response(&self) -> HttpResponse<actix_web::body::BoxBody> {
        HttpResponse::Unauthorized().json(self.message.clone())
    }
}
