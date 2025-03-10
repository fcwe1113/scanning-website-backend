use std::fmt;
use std::fmt::Formatter;

#[derive(Clone, Debug)]
pub(crate) enum ScreenState {
    TokenHandshake,
    Start,
    SignUp,
    StoreLocator,
    Scanner,
    Payment,
    Transfer,
    End
}

impl fmt::Display for ScreenState {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::TokenHandshake => write!(f, "Token Handshake"),
            Self::Start => write!(f, "Start"),
            Self::SignUp => write!(f, "SignUp"),
            Self::StoreLocator => write!(f, "Store Locator"),
            Self::Scanner => write!(f, "Scanner"),
            Self::Payment => write!(f, "Payment"),
            Self::Transfer => write!(f, "Transfer"),
            Self::End => write!(f, "End")
        }
    }
}

impl ScreenState {
    pub(crate) fn as_i32(&self) -> i32 {
        match self {
            Self::TokenHandshake => 0,
            Self::Start => 1,
            Self::SignUp => 2,
            Self::StoreLocator => 3,
            Self::Scanner => 4,
            Self::Payment => 5,
            Self::Transfer => 6,
            Self::End => 7
        }
    }
}