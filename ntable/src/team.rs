use std::fmt::{self, Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TeamNumber(u16);
impl TeamNumber {
    pub const fn new(team: u16) -> Option<Self> {
        if team < 25600 { Some(Self(team)) } else { None }
    }
    pub const fn new_unchecked(team: u16) -> Self {
        Self(team)
    }
    pub const fn team(self) -> u16 {
        self.0
    }
    pub const fn upper(self) -> u8 {
        (self.0 / 100) as u8
    }
    pub const fn lower(self) -> u8 {
        self.0 as u8 % 100
    }
    pub const fn to_ipv4(self) -> Ipv4Team {
        Ipv4Team(self)
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ipv4Team(TeamNumber);
impl Display for Ipv4Team {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "10.{}.{}.1", self.0.upper(), self.0.lower())
    }
}
