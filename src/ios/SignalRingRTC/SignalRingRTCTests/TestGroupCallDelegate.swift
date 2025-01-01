//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

import Foundation
import SignalRingRTC

class TestGroupCallDelegate: GroupCallDelegate {
    var requestMembershipProofCount = 0
    var requestGroupMembersCount = 0
    var onLocalDeviceStateChangedCount = 0
    var onRemoteDeviceStatesChangedCount = 0
    var onAudioLevelsCount = 0
    var onLowBandwidthForVideoCount = 0
    var onReactionsCount = 0
    var onRaisedHandsCount = 0
    var onPeekChangedCount = 0
    var onEndedCount = 0
    var onSpeakingCount = 0
    var lastOnEndedReason: GroupCallEndReason? = nil
    var lastOnSpeakingEvent: SpeechEvent? = nil

    func groupCall(requestMembershipProof groupCall: GroupCall) {
        requestMembershipProofCount += 1
    }

    func groupCall(requestGroupMembers groupCall: GroupCall) {
        requestGroupMembersCount += 1
    }

    func groupCall(onLocalDeviceStateChanged groupCall: GroupCall) {
        onLocalDeviceStateChangedCount += 1
    }

    func groupCall(onRemoteDeviceStatesChanged groupCall: GroupCall) {
        onRemoteDeviceStatesChangedCount += 1
    }

    func groupCall(onAudioLevels groupCall: GroupCall) {
        onAudioLevelsCount += 1
    }

    func groupCall(onLowBandwidthForVideo groupCall: GroupCall, recovered: Bool) {
        onLowBandwidthForVideoCount += 1
    }

    func groupCall(onReactions groupCall: GroupCall, reactions: [Reaction]) {
        onReactionsCount += 1
    }

    func groupCall(onRaisedHands groupCall: GroupCall, raisedHands: [UInt32]) {
        onRaisedHandsCount += 1
    }

    func groupCall(onPeekChanged groupCall: GroupCall) {
        onPeekChangedCount += 1
    }

    func groupCall(onEnded groupCall: GroupCall, reason: GroupCallEndReason) {
        onEndedCount += 1
        lastOnEndedReason = reason
    }

    func groupCall(onSpeakingNotification groupCall: GroupCall, event: SpeechEvent) {
        onSpeakingCount += 1
        lastOnSpeakingEvent = event
    }
}
