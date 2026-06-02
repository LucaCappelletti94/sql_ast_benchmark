//! Release cadence, shared by the parser and dialect metadata snapshots: how
//! often a new version ships, and whether the project looks dead.

/// How frequently a project publishes releases.
#[derive(Clone, Copy, PartialEq)]
pub enum Cadence {
    /// Continuously deployed, no discrete versioned releases (cloud services).
    Rolling,
    /// Roughly monthly or faster.
    Monthly,
    /// Every few months.
    Quarterly,
    /// About once a year.
    Yearly,
    /// Only every few years.
    Multiyear,
    /// No fixed schedule, or too few releases to tell.
    Irregular,
    /// No release in a long time: the project looks dormant or abandoned. Not
    /// currently assigned to any benchmarked project, but kept so a stale one is
    /// flagged the moment its snapshot is updated.
    #[allow(dead_code)]
    Dormant,
}

impl Cadence {
    /// Short label for the badge.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Cadence::Rolling => "rolling",
            Cadence::Monthly => "monthly",
            Cadence::Quarterly => "quarterly",
            Cadence::Yearly => "yearly",
            Cadence::Multiyear => "multi-year",
            Cadence::Irregular => "irregular",
            Cadence::Dormant => "dormant",
        }
    }

    /// Whether the cadence is healthy. Only [`Cadence::Dormant`] is flagged.
    #[must_use]
    pub const fn is_ok(self) -> bool {
        !matches!(self, Cadence::Dormant)
    }

    /// Full sentence for the tooltip.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Cadence::Rolling => {
                "Continuously deployed: new functionality ships in a rolling fashion, with no discrete version releases."
            }
            Cadence::Monthly => "New releases land roughly monthly or faster: very actively developed.",
            Cadence::Quarterly => "New releases land every few months: steadily maintained.",
            Cadence::Yearly => "Roughly one release a year.",
            Cadence::Multiyear => "A new version only every few years.",
            Cadence::Irregular => {
                "Releases on no fixed schedule, or too few exist to establish a cadence."
            }
            Cadence::Dormant => {
                "No release in a long time: the project looks dormant or abandoned."
            }
        }
    }
}
