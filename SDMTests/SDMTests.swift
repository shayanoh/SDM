//
//  SDMTests.swift
//  SDMTests
//
//  Created by Shayan Ostadhassan on 8/3/26.
//

import Testing

@testable import SDM

@Test func appTargetLinksAgainstSDMKit() {
    _ = EngineController.self
    #expect(Bool(true))
}
