pub fn is_xid_start(c: char) -> bool {
    unicode_ident::is_xid_start(c)
}

pub fn is_xid_continue(c: char) -> bool {
    unicode_ident::is_xid_continue(c)
}
