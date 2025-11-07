// Test modules for timing wheel
// 
// 时间轮测试模块

#[cfg(test)]
mod wheel {
    mod batch_tests;
    mod postpone_tests;
    mod periodic_tests;
    mod advanced_tests;
}

#[cfg(test)]
mod timer {
    mod cancel_tests;
    mod postpone_tests;
    mod periodic_tests;
    mod batch_tests;
}

#[cfg(test)]
mod service {
    mod cancel_tests;
    mod postpone_tests;
    mod periodic_tests;
    mod batch_tests;
}

