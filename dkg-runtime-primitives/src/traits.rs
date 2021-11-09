use sp_std::vec::Vec;

pub trait OnAuthoritySetChangeHandler<AuthoritySetId, AuthorityId> {
	fn on_authority_set_changed(
		authority_set_id: AuthoritySetId,
		authorities: Vec<AuthorityId>,
	) -> ();
}

impl<AuthoritySetId, AuthorityId> OnAuthoritySetChangeHandler<AuthoritySetId, AuthorityId> for () {
	fn on_authority_set_changed(
		_authority_set_id: AuthoritySetId,
		_authorities: Vec<AuthorityId>,
	) -> () {
	}
}
