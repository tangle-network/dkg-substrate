pub trait OnAuthoritySetChangeHandler<AuthoritySetId, AuthorityId> {
	fn on_authority_set_change(
		authority_set_id: AuthoritySetId,
		authorities: Vec<AuthorityId>,
	) -> ();
}

impl<AuthoritySetId, AuthorityId> OnAuthoritySetChangeHandler<AuthoritySetId, AuthorityId> for () {
	fn on_authority_set_change(
		_authority_Set_id: AuthoritySetId,
		_authorities: Vec<AuthorityId>,
	) -> () {
	}
}
