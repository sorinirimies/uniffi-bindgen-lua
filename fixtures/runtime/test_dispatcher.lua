-- Regression test of the shared dispatcher (see dispatcher.c).
assert(require("dispatcher").run(), "the callback did not run")
print("dispatcher: ok")
