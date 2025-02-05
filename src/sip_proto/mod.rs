use rsip::Method;
use rsip::typed::Allow;

pub mod options;
pub mod register;
pub mod sdp;
pub mod sip_message_decoder;

pub fn get_allow_header() -> Allow
{
    Allow::from(vec![Method::Invite, Method::Ack, Method::Bye, Method::Cancel, Method::Options])
}