// Test modules for timing wheel
//
// 时间轮测试模块

#[cfg(test)]
mod wheel {
    mod advanced_tests;
    mod batch_tests;
    mod periodic_tests;
    mod postpone_tests;
}

#[cfg(test)]
mod timer {
    mod batch_tests;
    mod cancel_tests;
    mod periodic_tests;
    mod postpone_tests;
}

#[cfg(test)]
mod service {
    mod batch_tests;
    mod cancel_tests;
    mod periodic_tests;
    mod postpone_tests;
}
