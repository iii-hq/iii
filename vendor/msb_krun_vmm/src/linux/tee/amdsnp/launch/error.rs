// SPDX-License-Identifier: Apache-2.0

use std::{
    convert::From,
    error,
    fmt::{Debug, Display},
    io,
};

use std::os::raw::c_int;

/// Error conditions returned by the SEV platform or by layers above it
/// (i.e., the Linux kernel).
///
/// These error conditions are documented in the AMD SEV API spec, but
/// their documentation has been copied here for completeness.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u32)]
pub enum SevError {
    /// The platform state is invalid for this command.
    InvalidPlatformState = 0x0001,

    /// The guest state is invalid for this command.
    InvalidGuestState = 0x0002,

    /// The platform configuration is invalid.
    InvalidConfig = 0x0003,

    /// A memory buffer is too small.
    InvalidLen = 0x0004,

    /// The platform is already owned.
    AlreadyOwned = 0x0005,

    /// The certificate is invalid.
    InvalidCertificate = 0x0006,

    /// Request is not allowed by guest policy.
    PolicyFailure = 0x0007,

    /// The guest is inactive.
    Inactive = 0x0008,

    /// The address provided is invalid.
    InvalidAddress = 0x0009,

    /// The provided signature is invalid.
    BadSignature = 0x000A,

    /// The provided measurement is invalid.
    BadMeasurement = 0x000B,

    /// The ASID is already owned.
    AsidOwned = 0x000C,

    /// The ASID is invalid.
    InvalidAsid = 0x000D,

    /// WBINVD instruction required.
    WbinvdRequired = 0x000E,

    /// `DF_FLUSH` invocation required.
    DfFlushRequired = 0x000F,

    /// The guest handle is invalid.
    InvalidGuest = 0x0010,

    /// The command issued is invalid.
    InvalidCommand = 0x0011,

    /// The guest is active.
    Active = 0x0012,

    /// A hardware condition has occurred affecting the platform. It is safe
    /// to re-allocate parameter buffers.
    HardwarePlatform = 0x0013,

    /// A hardware condition has occurred affecting the platform. Re-allocating
    /// parameter buffers is not safe.
    HardwareUnsafe = 0x0014,

    /// Feature is unsupported.
    Unsupported = 0x0015,

    /// A given parameter is invalid.
    InvalidParam = 0x0016,

    /// The SEV firmware has run out of a resource required to carry out the
    /// command.
    ResourceLimit = 0x0017,

    /// The SEV platform observed a failed integrity check.
    SecureDataInvalid = 0x0018,

    /// The RMP page size is incorrect.
    InvalidPageSize = 0x0019,

    /// The RMP page state is incorrect
    InvalidPageState = 0x001A,

    /// The metadata entry is invalid.
    InvalidMdataEntry = 0x001B,

    /// The page ownership is incorrect
    InvalidPageOwner = 0x001C,

    /// The AEAD algorithm would have overflowed
    AEADOFlow = 0x001D,

    /// A Mailbox mode command was sent while the SEV FW was in Ring Buffer
    /// mode. Ring Buffer mode has been exited; the Mailbox mode command
    /// has been ignored. Retry is recommended.
    RbModeExited = 0x001F, // 0x001F

    /// The RMP must be reinitialized.
    RMPInitRequired = 0x0020, // 0x0020

    /// SVN of provided image is lower than the committed SVN.
    BadSvn = 0x0021,

    /// Firmware version anti-rollback.
    BadVersion = 0x0022,

    /// An invocation of SNP_SHUTDOWN is required to complete this action.
    ShutdownRequired = 0x0023,

    /// Update of the firmware internal state or a guest context page has failed.
    UpdateFailed = 0x0024,

    /// Installation of the committed firmware image required
    RestoreRequired = 0x0025,

    /// The RMP initialization failed.
    RMPInitFailed = 0x0026,

    /// The key requested is invalid, not present, or not allowed.
    InvalidKey = 0x0027,

    /// Unknown status code
    UnknownError = 0x0000,
}

impl std::fmt::Display for SevError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let code = *self as u32;
        match self {
            SevError::InvalidPlatformState => {
                write!(f, "Status Code: 0x{:x}: Invalid platform state.", code)
            }
            SevError::InvalidGuestState => {
                write!(f, "Status Code: 0x{:x}: Invalid guest state.", code)
            }
            SevError::InvalidConfig => write!(
                f,
                "Status Code: 0x{:x}: Platform configuration invalid.",
                code
            ),
            SevError::InvalidLen => {
                write!(f, "Status Code: 0x{:x}: Memory buffer too small.", code)
            }
            SevError::AlreadyOwned => {
                write!(f, "Status Code: 0x{:x}: Platform is already owned.", code)
            }
            SevError::InvalidCertificate => {
                write!(f, "Status Code: 0x{:x}: Invalid certificate.", code)
            }
            SevError::PolicyFailure => write!(f, "Status Code: 0x{:x}: Policy failure.", code),
            SevError::Inactive => write!(f, "Status Code: 0x{:x}: Guest is inactive.", code),
            SevError::InvalidAddress => {
                write!(f, "Status Code: 0x{:x}: Provided address is invalid.", code)
            }
            SevError::BadSignature => write!(
                f,
                "Status Code: 0x{:x}: Provided signature is invalid.",
                code
            ),
            SevError::BadMeasurement => write!(
                f,
                "Status Code: 0x{:x}: Provided measurement is invalid.",
                code
            ),
            SevError::AsidOwned => write!(f, "Status Code: 0x{:x}: ASID is already owned.", code),
            SevError::InvalidAsid => write!(f, "Status Code: 0x{:x}: ASID is invalid.", code),
            SevError::WbinvdRequired => {
                write!(f, "Status Code: 0x{:x}: WBINVD instruction required.", code)
            }
            SevError::DfFlushRequired => write!(
                f,
                "Status Code: 0x{:x}: DF_FLUSH invocation required.",
                code
            ),
            SevError::InvalidGuest => {
                write!(f, "Status Code: 0x{:x}: Guest handle is invalid.", code)
            }
            SevError::InvalidCommand => {
                write!(f, "Status Code: 0x{:x}: Issued command is invalid.", code)
            }
            SevError::Active => write!(f, "Status Code: 0x{:x}: Guest is active.", code),
            SevError::HardwarePlatform => {
                write!(
                    f,
                    "Status Code: 0x{:x}: Hardware condition occured, safe to re-allocate parameter buffers.",
                    code
                )
            }
            SevError::HardwareUnsafe => {
                write!(
                    f,
                    "Status Code: 0x{:x}: Hardware condition occured, unsafe to re-allocate parameter buffers.",
                    code
                )
            }
            SevError::Unsupported => {
                write!(f, "Status Code: 0x{:x}: Feature is unsupported.", code)
            }
            SevError::InvalidParam => {
                write!(f, "Status Code: 0x{:x}: Given parameter is invalid.", code)
            }
            SevError::ResourceLimit => {
                write!(
                    f,
                    "Status Code: 0x{:x}: SEV firmware has run out of required resources to carry out command.",
                    code
                )
            }
            SevError::SecureDataInvalid => write!(
                f,
                "Status Code: 0x{:x}: SEV platform observed a failed integrity check.",
                code
            ),
            SevError::InvalidPageSize => write!(
                f,
                "Status Code: 0x{:x}: The RMP page size is incorrect.",
                code
            ),
            SevError::InvalidPageState => write!(
                f,
                "Status Code: 0x{:x}: The RMP page state is incorrect.",
                code
            ),
            SevError::InvalidMdataEntry => write!(
                f,
                "Status Code: 0x{:x}: The metadata entry is invalid.",
                code
            ),
            SevError::InvalidPageOwner => write!(
                f,
                "Status Code: 0x{:x}: The page ownership is incorrect.",
                code
            ),
            SevError::AEADOFlow => write!(
                f,
                "Status Code: 0x{:x}: The AEAD algorithm would have overflowed.",
                code
            ),
            SevError::RbModeExited => write!(
                f,
                "Status Code: 0x{:x}: A Mailbox mode command was sent while the SEV FW was in Ring Buffer \
                                    mode. Ring Buffer mode has been exited; the Mailbox mode command has \
                                    been ignored. Retry is recommended.",
                code
            ),
            SevError::RMPInitRequired => write!(
                f,
                "Status Code: 0x{:x}: The RMP must be reinitialized.",
                code
            ),
            SevError::BadSvn => write!(
                f,
                "Status Code: 0x{:x}: SVN of provided image is lower than the committed SVN.",
                code
            ),
            SevError::BadVersion => write!(
                f,
                "Status Code: 0x{:x}: Firmware version anti-rollback.",
                code
            ),
            SevError::ShutdownRequired => write!(
                f,
                "Status Code: 0x{:x}: An invocation of SNP_SHUTDOWN is required to complete this action.",
                code
            ),
            SevError::UpdateFailed => write!(
                f,
                "Status Code: 0x{:x}: Update of the firmware internal state or a guest context page has failed.",
                code
            ),
            SevError::RestoreRequired => write!(
                f,
                "Status Code: 0x{:x}: Installation of the committed firmware image required.",
                code
            ),
            SevError::RMPInitFailed => write!(
                f,
                "Status Code: 0x{:x}: The RMP initialization failed.",
                code
            ),
            SevError::InvalidKey => write!(
                f,
                "Status Code: 0x{:x}: The key requested is invalid, not present, or not allowed.",
                code
            ),
            SevError::UnknownError => write!(f, "Unknown SEV Error"),
        }
    }
}

impl From<u64> for SevError {
    fn from(value: u64) -> Self {
        Self::from(value as u32)
    }
}

impl From<u32> for SevError {
    #[inline]
    fn from(error: u32) -> SevError {
        match error {
            0x01 => SevError::InvalidPlatformState,
            0x02 => SevError::InvalidGuestState,
            0x03 => SevError::InvalidConfig,
            0x04 => SevError::InvalidLen,
            0x05 => SevError::AlreadyOwned,
            0x06 => SevError::InvalidCertificate,
            0x07 => SevError::PolicyFailure,
            0x08 => SevError::Inactive,
            0x09 => SevError::InvalidAddress,
            0x0A => SevError::BadSignature,
            0x0B => SevError::BadMeasurement,
            0x0C => SevError::AsidOwned,
            0x0D => SevError::InvalidAsid,
            0x0E => SevError::WbinvdRequired,
            0x0F => SevError::DfFlushRequired,
            0x10 => SevError::InvalidGuest,
            0x11 => SevError::InvalidCommand,
            0x12 => SevError::Active,
            0x13 => SevError::HardwarePlatform,
            0x14 => SevError::HardwareUnsafe,
            0x15 => SevError::Unsupported,
            0x16 => SevError::InvalidParam,
            0x17 => SevError::ResourceLimit,
            0x18 => SevError::SecureDataInvalid,
            0x19 => SevError::InvalidPageSize,
            0x1A => SevError::InvalidPageState,
            0x1B => SevError::InvalidMdataEntry,
            0x1C => SevError::InvalidPageOwner,
            0x1D => SevError::AEADOFlow,
            0x1F => SevError::RbModeExited,
            0x20 => SevError::RMPInitRequired,
            0x21 => SevError::BadSvn,
            0x22 => SevError::BadVersion,
            0x23 => SevError::ShutdownRequired,
            0x24 => SevError::UpdateFailed,
            0x25 => SevError::RestoreRequired,
            0x26 => SevError::RMPInitFailed,
            0x27 => SevError::InvalidKey,
            _ => SevError::UnknownError,
        }
    }
}

impl From<SevError> for c_int {
    fn from(err: SevError) -> Self {
        match err {
            SevError::InvalidPlatformState => 0x01,
            SevError::InvalidGuestState => 0x02,
            SevError::InvalidConfig => 0x03,
            SevError::InvalidLen => 0x04,
            SevError::AlreadyOwned => 0x05,
            SevError::InvalidCertificate => 0x06,
            SevError::PolicyFailure => 0x07,
            SevError::Inactive => 0x08,
            SevError::InvalidAddress => 0x09,
            SevError::BadSignature => 0x0A,
            SevError::BadMeasurement => 0x0B,
            SevError::AsidOwned => 0x0C,
            SevError::InvalidAsid => 0x0D,
            SevError::WbinvdRequired => 0x0E,
            SevError::DfFlushRequired => 0x0F,
            SevError::InvalidGuest => 0x10,
            SevError::InvalidCommand => 0x11,
            SevError::Active => 0x12,
            SevError::HardwarePlatform => 0x13,
            SevError::HardwareUnsafe => 0x14,
            SevError::Unsupported => 0x15,
            SevError::InvalidParam => 0x16,
            SevError::ResourceLimit => 0x17,
            SevError::SecureDataInvalid => 0x18,
            SevError::InvalidPageSize => 0x19,
            SevError::InvalidPageState => 0x1A,
            SevError::InvalidMdataEntry => 0x1B,
            SevError::InvalidPageOwner => 0x1C,
            SevError::AEADOFlow => 0x1D,
            SevError::RbModeExited => 0x1F,
            SevError::RMPInitRequired => 0x20,
            SevError::BadSvn => 0x21,
            SevError::BadVersion => 0x22,
            SevError::ShutdownRequired => 0x23,
            SevError::UpdateFailed => 0x24,
            SevError::RestoreRequired => 0x25,
            SevError::RMPInitFailed => 0x26,
            SevError::InvalidKey => 0x27,
            SevError::UnknownError => -1,
        }
    }
}

impl std::error::Error for SevError {}

/// There are a number of error conditions that can occur between this
/// layer all the way down to the SEV platform. Most of these cases have
/// been enumerated; however, there is a possibility that some error
/// conditions are not encapsulated here.
#[derive(Debug)]
pub enum FirmwareError {
    /// The error condition is known.
    KnownSev(SevError),

    /// The error condition is unknown.
    UnknownSev(u32),

    /// IO Error
    Io(std::io::Error),
}

impl error::Error for FirmwareError {}

impl Display for FirmwareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let err_description = match self {
            FirmwareError::KnownSev(error) => format!("Known SEV FW Error: {error}"),
            FirmwareError::UnknownSev(code) => {
                format!("Unknown SEV FW Error Encountered: {code}")
            }
            FirmwareError::Io(error) => format!("IO Error Encountered: {error}"),
        };

        write!(f, "{err_description}")
    }
}

impl std::convert::From<SevError> for FirmwareError {
    fn from(sev_error: SevError) -> Self {
        match sev_error {
            SevError::UnknownError => FirmwareError::UnknownSev(sev_error as u32),
            _ => FirmwareError::KnownSev(sev_error),
        }
    }
}

impl From<io::Error> for FirmwareError {
    #[inline]
    fn from(error: io::Error) -> FirmwareError {
        FirmwareError::Io(error)
    }
}

impl From<u64> for FirmwareError {
    fn from(value: u64) -> Self {
        Self::from(value as u32)
    }
}

impl From<u32> for FirmwareError {
    #[inline]
    fn from(error: u32) -> FirmwareError {
        match error {
            0x00 => FirmwareError::Io(io::Error::last_os_error()),
            0x01..0x027 => FirmwareError::KnownSev(error.into()),
            _ => FirmwareError::UnknownSev(error),
        }
    }
}

impl From<FirmwareError> for c_int {
    fn from(err: FirmwareError) -> Self {
        match err {
            FirmwareError::UnknownSev(_) | FirmwareError::Io(_) => -0x01,
            FirmwareError::KnownSev(e) => e.into(),
        }
    }
}
