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
    fn as_i32(&self) -> i32 {
        match self {
            Self::TokenHandshake => 1,
            Self::Start => 2,
            Self::SignUp => 3,
            Self::StoreLocator => 4,
            Self::Scanner => 5,
            Self::Payment => 6,
            Self::Transfer => 7,
            Self::End => 8
        }
    }
}