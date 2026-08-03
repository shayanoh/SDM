//
//  SDMTests.swift
//  SDMTests
//
//  Created by Shayan Ostadhassan on 8/3/26.
//

import Testing

@testable import SDM

/// The assertion here is that this file compiles and the target links: naming
/// `EngineController` forces the app target to resolve `SDMEngine` and
/// `SDMCore` through the local package, so a broken package reference fails
/// the build rather than surfacing at launch.
///
/// There is deliberately no `#expect`. The one that used to be here,
/// `#expect(Bool(true))`, was a test that could not fail.
@Test func appTargetLinksAgainstSDMKit() {
    _ = EngineController.self
}
