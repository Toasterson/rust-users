#![crate_name = "users"]
#![crate_type = "rlib"]
#![crate_type = "dylib"]

//! This is a library for getting information on Unix users and groups.
//!
//! In Unix, each user has an individual *user ID*, and each process has an
//! *effective user ID* that says which user's permissions it is using.
//! Furthermore, users can be the members of *groups*, which also have names
//! and IDs. This functionality is exposed in libc, the C standard library,
//! but as this an unsafe Rust interface. This wrapper library provides a safe
//! interface, using User and Group objects instead of low-level pointers and
//! strings. It also offers basic caching functionality.
//!
//! It does not (yet) offer *editing* functionality; the objects returned are
//! read-only.
//!
//! Users
//! -----
//!
//! The function `get_current_uid` returns a `i32` value representing the user
//! currently running the program, and the `get_user_by_uid` function scans the
//! users database and returns a User object with the user's information. This
//! function returns `None` when there is no user for that ID.
//!
//! A `User` object has the following public fields:
//!
//! - **uid:** The user's ID
//! - **name:** The user's name
//! - **primary_group:** The ID of this user's primary group
//!
//! Here is a complete example that prints out the current user's name:
//!
//! ```rust
//! use users::{get_user_by_uid, get_current_uid};
//! let user = get_user_by_uid(get_current_uid()).unwrap();
//! println!("Hello, {}!", user.name);
//! ```
//!
//! This code assumes (with `unwrap()`) that the user hasn't been deleted
//! after the program has started running. For arbitrary user IDs, this is
//! **not** a safe assumption: it's possible to delete a user while it's
//! running a program, or is the owner of files, or for that user to have
//! never existed. So always check the return values from `user_to_uid`!
//!
//! Caching
//! -------
//!
//! Despite the above warning, the users and groups database rarely changes.
//! While a short program may only need to get user information once, a
//! long-running one may need to re-query the database many times, and a
//! medium-length one may get away with caching the values to save on redundant
//! system calls.
//!
//! For this reason, this crate offers a caching interface to the database,
//! which offers the same functionality while holding on to every result,
//! caching the information so it can be re-used.
//!
//! To introduce a cache, create a new `OSUsers` object and call the same
//! methods on it. For example:
//!
//! ```rust
//! use users::{Users, OSUsers};
//! let mut cache = OSUsers::empty_cache();
//! let uid = cache.get_current_uid();
//! let user = cache.get_user_by_uid(uid).unwrap();
//! println!("Hello again, {}!", user.name);
//! ```
//!
//! This cache is **only additive**: it's not possible to drop it, or erase
//! selected entries, as when the database may have been modified, it's best to
//! start entirely afresh. So to accomplish this, just start using a new
//! `OSUsers` object.
//!
//! Groups
//! ------
//!
//! Finally, it's possible to get groups in a similar manner. A `Group` object
//! has the following public fields:
//!
//! - **gid:** The group's ID
//! - **name:** The group's name
//! - **members:** Vector of names of the users that belong to this group
//!
//! And again, a complete example:
//!
//! ```rust
//! use users::{Users, OSUsers};
//! let mut cache = OSUsers::empty_cache();
//! let group = cache.get_group_by_name("admin".to_string()).expect("No such group 'admin'!");
//! println!("The '{}' group has the ID {}", group.name, group.gid);
//! for member in group.members.into_iter() {
//!     println!("{} is a member of the group", member);
//! }
//! ```

extern crate libc;
use libc::{c_char, c_int, uid_t, gid_t, time_t};

use std::ptr::read;
use std::collections::HashMap;
use std::collections::hash_map::Entry::{Occupied, Vacant};

/// The trait for the `OSUsers` object.
pub trait Users {

    /// Return a User object if one exists for the given user ID; otherwise, return None.
    fn get_user_by_uid(&mut self, uid: i32) -> Option<User>;

    /// Return a User object if one exists for the given username; otherwise, return None.
    fn get_user_by_name(&mut self, username: String) -> Option<User>;

    /// Return a Group object if one exists for the given group ID; otherwise, return None.
    fn get_group_by_gid(&mut self, gid: u32) -> Option<Group>;

    /// Return a Group object if one exists for the given groupname; otherwise, return None.
    fn get_group_by_name(&mut self, group_name: String) -> Option<Group>;

    /// Return the user ID for the user running the program.
    fn get_current_uid(&mut self) -> i32;
}

#[repr(C)]
struct c_passwd {
    pub pw_name:    *const c_char,  // login name
    pub pw_passwd:  *const c_char,
    pub pw_uid:     c_int,          // user ID
    pub pw_gid:     c_int,          // group ID
    pub pw_change:  time_t,
    pub pw_class:   *const c_char,
    pub pw_gecos:   *const c_char,  // full name
    pub pw_dir:     *const c_char,  // login dir
    pub pw_shell:   *const c_char,  // login shell
    pub pw_expire:  time_t,         // password expiry time
}

impl Copy for c_passwd { }

#[repr(C)]
struct c_group {
    pub gr_name:   *const c_char,         // group name
    pub gr_passwd: *const c_char,         // password
    pub gr_gid:    gid_t,                 // group id
    pub gr_mem:    *const *const c_char,  // names of users in the group
}

impl Copy for c_group { }

extern {
    fn getpwuid(uid: c_int) -> *const c_passwd;
    fn getpwnam(user_name: *const c_char) -> *const c_passwd;

    fn getgrgid(gid: uid_t) -> *const c_group;
    fn getgrnam(group_name: *const c_char) -> *const c_group;

    fn getuid() -> c_int;
}

#[deriving(Clone)]
/// Information about a particular user.
pub struct User {

    /// This user's ID
    pub uid: i32,

    /// This user's name
    pub name: String,

    /// The ID of this user's primary group
    pub primary_group: u32,
}

/// Information about a particular group.
#[deriving(Clone)]
pub struct Group {

    /// This group's ID
    pub gid: u32,

    /// This group's name
    pub name: String,

    /// Vector of the names of the users who belong to this group as a non-primary member
    pub members: Vec<String>,
}

/// A producer of user and group instances that caches every result.
#[deriving(Clone)]
pub struct OSUsers {
    users: HashMap<i32, Option<User>>,
    users_back: HashMap<String, Option<i32>>,

    groups: HashMap<u32, Option<Group>>,
    groups_back: HashMap<String, Option<u32>>,

    uid: Option<i32>,
}

unsafe fn passwd_to_user(pointer: *const c_passwd) -> Option<User> {
    if pointer.is_not_null() {
        let pw = read(pointer);
        Some(User { uid: pw.pw_uid, name: String::from_raw_buf(pw.pw_name as *const u8), primary_group: pw.pw_gid as u32 })
    }
    else {
        None
    }
}

unsafe fn struct_to_group(pointer: *const c_group) -> Option<Group> {
    if pointer.is_not_null() {
        let gr = read(pointer);
        let name = String::from_raw_buf(gr.gr_name as *const u8);
        let members = members(gr.gr_mem);
        Some(Group { gid: gr.gr_gid, name: name, members: members })
    }
    else {
        None
    }
}

unsafe fn members(groups: *const *const c_char) -> Vec<String> {
    let mut i = 0;
    let mut members = vec![];

    // The list of members is a pointer to a pointer of
    // characters, terminated by a null pointer.
    loop {
        match groups.offset(i).as_ref() {
            Some(&username) => {
                if username.is_not_null() {
                    members.push(String::from_raw_buf(username as *const u8));
                }
                else {
                    return members;
                }

                i += 1;
            },

            // This should never happen, but if it does, this is the
            // sensible thing to do
            None => return members,
        }
    }
}

impl Users for OSUsers {
    fn get_user_by_uid(&mut self, uid: i32) -> Option<User> {
        match self.users.entry(uid) {
            Vacant(entry) => {
                let user = unsafe { passwd_to_user(getpwuid(uid as i32)) };
                match user {
                    Some(user) => {
                        entry.set(Some(user.clone()));
                        self.users_back.insert(user.name.clone(), Some(user.uid));
                        Some(user)
                    },
                    None => {
                        entry.set(None);
                        None
                    }
                }
            },
            Occupied(entry) => entry.get().clone(),
        }
    }

    fn get_user_by_name(&mut self, username: String) -> Option<User> {
        match self.users_back.entry(username.clone()) {
            Vacant(entry) => {
                let user = unsafe { passwd_to_user(getpwnam(username.as_ptr() as *const i8)) };
                match user {
                    Some(user) => {
                        entry.set(Some(user.uid));
                        self.users.insert(user.uid, Some(user.clone()));
                        Some(user)
                    },
                    None => {
                        entry.set(None);
                        None
                    }
                }
            },
            Occupied(entry) => match entry.get() {
                &Some(uid) => self.users[uid].clone(),
                &None => None,
            }
        }
    }

    fn get_group_by_gid(&mut self, gid: u32) -> Option<Group> {
        match self.groups.clone().entry(gid) {
            Vacant(entry) => {
                let group = unsafe { struct_to_group(getgrgid(gid)) };
                match group {
                    Some(group) => {
                        entry.set(Some(group.clone()));
                        self.groups_back.insert(group.name.clone(), Some(group.gid));
                        Some(group)
                    },
                    None => {
                        entry.set(None);
                        None
                    }
                }
            },
            Occupied(entry) => entry.get().clone(),
        }
    }

    fn get_group_by_name(&mut self, group_name: String) -> Option<Group> {
        match self.groups_back.clone().entry(group_name.clone()) {
            Vacant(entry) => {
                let user = unsafe { struct_to_group(getgrnam(group_name.as_ptr() as *const i8)) };
                match user {
                    Some(group) => {
                        entry.set(Some(group.gid));
                        self.groups.insert(group.gid, Some(group.clone()));
                        Some(group)
                    },
                    None => {
                        entry.set(None);
                        None
                    }
                }
            },
            Occupied(entry) => match entry.get() {
                &Some(gid) => self.groups[gid].clone(),
                &None => None,
            }
        }
    }

    fn get_current_uid(&mut self) -> i32 {
        match self.uid {
            Some(uid) => uid,
            None => {
                let uid = unsafe { getuid() };
                self.uid = Some(uid);
                uid
            }
        }
    }
}

impl OSUsers {
    /// Create a new empty OS Users object.
    pub fn empty_cache() -> OSUsers {
        OSUsers {
            users:       HashMap::new(),
            users_back:  HashMap::new(),
            groups:      HashMap::new(),
            groups_back: HashMap::new(),
            uid:         None,
        }
    }
}

/// Return a User object if one exists for the given user ID; otherwise, return None.
pub fn get_user_by_uid(uid: i32) -> Option<User> {
    OSUsers::empty_cache().get_user_by_uid(uid)
}

/// Return a User object if one exists for the given username; otherwise, return None.
pub fn get_user_by_name(username: String) -> Option<User> {
    OSUsers::empty_cache().get_user_by_name(username)
}

/// Return a Group object if one exists for the given group ID; otherwise, return None.
pub fn get_group_by_gid(gid: u32) -> Option<Group> {
    OSUsers::empty_cache().get_group_by_gid(gid)
}

/// Return a Group object if one exists for the given groupname; otherwise, return None.
pub fn get_group_by_name(group_name: String) -> Option<Group> {
    OSUsers::empty_cache().get_group_by_name(group_name)
}

/// Return the user ID for the user running the program.
pub fn get_current_uid() -> i32 {
    OSUsers::empty_cache().get_current_uid()
}

#[cfg(test)]
mod test {
    use super::{Users, OSUsers};

    #[test]
    fn uid() {
        OSUsers::empty_cache().get_current_uid();
    }

    #[test]
    fn username() {
        let mut users = OSUsers::empty_cache();
        let uid = users.get_current_uid();
        users.get_user_by_uid(uid);
    }

    #[test]
    fn uid_for_username() {
        let mut users = OSUsers::empty_cache();
        let uid = users.get_current_uid();
        let user = users.get_user_by_uid(uid).unwrap();
        assert_eq!(user.uid, uid);
    }

    #[test]
    fn username_for_uid_for_username() {
        let mut users = OSUsers::empty_cache();
        let uid = users.get_current_uid();
        let user = users.get_user_by_uid(uid).unwrap();
        let user2 = users.get_user_by_uid(user.uid).unwrap();
        assert_eq!(user2.uid, uid);
    }
}