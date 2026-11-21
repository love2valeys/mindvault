// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @title Registry
/// @notice Registry-level maintenance status with an auditable, bounded reason code.
/// @dev Distinguishes scheduled pause from emergency pause. Reads remain available
///      while paused; only authorized administrators may change status.
contract Registry {
    /// @notice Maintenance status of the registry.
    /// @dev ACTIVE = normal operation, SCHEDULED = planned pause, EMERGENCY = urgent pause.
    enum MaintenanceStatus {
        ACTIVE,
        SCHEDULED,
        EMERGENCY
    }

    /// @notice Bounded reason codes compatible with the maintenance status.
    /// @dev 0 = NONE (required for ACTIVE), 1 = UPGRADE, 2 = INCIDENT, 3 = AUDIT, 4 = MIGRATION.
    uint8 public constant REASON_NONE = 0;
    uint8 public constant REASON_UPGRADE = 1;
    uint8 public constant REASON_INCIDENT = 2;
    uint8 public constant REASON_AUDIT = 3;
    uint8 public constant REASON_MIGRATION = 4;
    uint8 public constant MAX_REASON_CODE = 4;

    address public admin;
    MaintenanceStatus public maintenanceStatus;
    uint8 public maintenanceReasonCode;

    event AdminChanged(address indexed previousAdmin, address indexed newAdmin);
    event MaintenanceStatusChanged(
        MaintenanceStatus indexed previousStatus,
        MaintenanceStatus indexed newStatus,
        uint8 reasonCode,
        address indexed changedBy
    );

    error NotAdmin();
    error InvalidReasonCode(uint8 reasonCode);
    error ReasonCodeNotAllowed(MaintenanceStatus status, uint8 reasonCode);

    modifier onlyAdmin() {
        if (msg.sender != admin) revert NotAdmin();
        _;
    }

    constructor(address initialAdmin) {
        require(initialAdmin != address(0), "admin zero");
        admin = initialAdmin;
        maintenanceStatus = MaintenanceStatus.ACTIVE;
        maintenanceReasonCode = REASON_NONE;
        emit AdminChanged(address(0), initialAdmin);
    }

    /// @notice Transfer administrator rights.
    function setAdmin(address newAdmin) external onlyAdmin {
        require(newAdmin != address(0), "admin zero");
        emit AdminChanged(admin, newAdmin);
        admin = newAdmin;
    }

    /// @notice Update the registry maintenance status with a bounded reason code.
    /// @dev Only the administrator may call. ACTIVE requires REASON_NONE; paused
    ///      statuses require a non-zero reason code within MAX_REASON_CODE.
    function setMaintenanceStatus(MaintenanceStatus newStatus, uint8 reasonCode) external onlyAdmin {
        if (reasonCode > MAX_REASON_CODE) revert InvalidReasonCode(reasonCode);
        if (newStatus == MaintenanceStatus.ACTIVE) {
            if (reasonCode != REASON_NONE) revert ReasonCodeNotAllowed(newStatus, reasonCode);
        } else {
            if (reasonCode == REASON_NONE) revert ReasonCodeNotAllowed(newStatus, reasonCode);
        }

        MaintenanceStatus previous = maintenanceStatus;
        maintenanceStatus = newStatus;
        maintenanceReasonCode = reasonCode;
        emit MaintenanceStatusChanged(previous, newStatus, reasonCode, msg.sender);
    }

    /// @notice Whether the registry is currently paused for maintenance.
    function isPaused() external view returns (bool) {
        return maintenanceStatus != MaintenanceStatus.ACTIVE;
    }

    /// @notice Read the current maintenance status and reason code.
    /// @dev Reads remain available while paused.
    function maintenanceInfo() external view returns (MaintenanceStatus status, uint8 reasonCode) {
        return (maintenanceStatus, maintenanceReasonCode);
    }
}
