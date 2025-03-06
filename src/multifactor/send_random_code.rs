use std::{future::Future, time::SystemTime};

use actix_session::{Session, SessionExt};
use actix_web::HttpRequest;
use serde::{Deserialize, Serialize};

use super::{CheckCodeError, Factor, GenerateCodeError};

const MFA_RANDOM_CODE_KEY: &str = "mfa_random_code";

pub trait CodeSender {
    type Error: std::error::Error + 'static;
    fn send_code(&self, random_code: RandomCode) -> Result<(), Self::Error>;
}

#[derive(Deserialize, Serialize, Clone)]
pub struct RandomCode {
    value: String,
    valid_until: SystemTime,
}

impl RandomCode {
    pub fn new(value: &str, valid_until: SystemTime) -> Self {
        Self {
            value: value.to_owned(),
            valid_until,
        }
    }

    pub fn value(&self) -> &str {
        &self.value
    }
    pub fn valid_until(&self) -> &SystemTime {
        &self.valid_until
    }
}

pub struct MfaRandomCode<T: CodeSender> {
    code_generator: fn() -> RandomCode,
    code_sender: T,
}

impl<T: CodeSender> MfaRandomCode<T> {
    pub fn new(code_generator: fn() -> RandomCode, code_sender: T) -> Self {
        Self {
            code_generator,
            code_sender

        }
    }
}


fn cleanup_and_unknown_error(session: &Session, msg: &str, e: impl std::error::Error + 'static) -> GenerateCodeError {
    session.purge();
    GenerateCodeError::new_with_cause(msg, e)
}

fn cleanup_and_unknown_code_error(session: &Session, msg: &str) -> CheckCodeError {
    session.purge();
    CheckCodeError::UnknownError(msg.to_owned())
}
fn cleanup_and_time_is_up_error(session: &Session) -> CheckCodeError {
    session.purge();
    CheckCodeError::TimeIsUp("Code is no longer valid".to_owned())
}

impl<T: CodeSender> Factor for MfaRandomCode<T> {
    fn generate_code(&self, req: &HttpRequest) -> Result<(), GenerateCodeError> {
        let random_code = (self.code_generator)();
        let session = req.get_session();

        session.insert(MFA_RANDOM_CODE_KEY, random_code.clone())
            .map_err(|e| cleanup_and_unknown_error(&session, "Could not insert mfa code into session", e))?;

        self.code_sender.send_code(random_code)
            .map_err(|e| cleanup_and_unknown_error(&session,"Could not send code to user", e))?;

        Ok(())
    }

    fn get_unique_id(&self) -> String {
        "RNDCODE".to_owned()
    }

    fn check_code(
        &self,
        code: &str,
        req: &HttpRequest,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<(), CheckCodeError>>>> {
        let session = req.get_session();
        let owned_code = code.to_owned();
        
        Box::pin(async move {
            let random_code = session.get::<RandomCode>(MFA_RANDOM_CODE_KEY)
                .map_err(|_| cleanup_and_unknown_code_error(&session, "Could not load random code from session"))?;

            if let Some(random_code) = random_code {
                let now = SystemTime::now();
                if &now >= random_code.valid_until() {
                    return Err(cleanup_and_time_is_up_error(&session))
                }

                if owned_code != random_code.value() {
                    // ToDo: here we need to cound the attempts and reject finally with cleanup
                    return Err(CheckCodeError::InvalidCode);
                }

                Ok(())
            } else {
                Err(cleanup_and_unknown_code_error(&session, "No random code in session"))            
            }
        })
    }
}