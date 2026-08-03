//
//  Item.swift
//  SDM
//
//  Created by Shayan Ostadhassan on 8/3/26.
//

import Foundation
import SwiftData

@Model
final class Item {
    var timestamp: Date
    
    init(timestamp: Date) {
        self.timestamp = timestamp
    }
}
