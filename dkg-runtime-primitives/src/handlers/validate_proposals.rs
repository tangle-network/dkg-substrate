use scale_info::prelude::string::String;

#[derive(Debug, PartialEq, Eq)]
pub enum ValidationError {
	InvalidParameter(String),
	UnimplementedProposalKind,
}
