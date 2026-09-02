//
//  Helpers.swift
//  SDM
//
//  Created by Shayan Ostadhassan on 8/23/26.
//
import Foundation

func calculateTimeRemaningForBytes(_ bytes: Int64, atSpeedBPS: Double) -> TimeInterval {
    return Double(bytes) / atSpeedBPS
}

func formatTimeIntervalForEta(_ eta: TimeInterval) -> String {
    let etaSeconds = Int(eta) % 60
    let etaMinutes = Int(eta / 60) % 60
    let etaHours = Int(eta / 60 / 60) % 24
    let etaDays = Int(eta / 60 / 60 / 24)
    var result: String
    if etaDays > 0 {
        result = "About \(etaDays) Day\(etaDays > 1 ? "s":"")"
        if etaHours > 0 {
            result += " and \(etaHours) Hour\(etaHours > 1 ? "s":"")"
        }
    } else if etaHours > 0 {
        result = "About \(etaHours) Hour\(etaHours > 1 ? "s":"")"
        if etaMinutes > 5 {
            let etaMinutesRounded = Int(etaMinutes / 5) * 5
            result += " and \(etaMinutesRounded) Minutes"
        }
    } else if etaMinutes > 0 {
        result = "About \(etaMinutes) Minute\(etaMinutes > 1 ? "s":"")"
        if etaMinutes < 10 && etaSeconds > 15 {
            let etaSecondsRounded = Int(etaSeconds / 5) * 5
            result += " and \(etaSecondsRounded) Seconds"
        }
    } else if etaSeconds > 0 {
        if etaSeconds < 10 {
            result = "A few seconds"
        } else {
            result = "Less than a minute"
        }
    } else {
        result = "Soon"
    }
    return result
}
