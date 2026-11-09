/* Regression test: the modules of one Lua state share a dispatcher.
 *
 * A Lua program using two components (an MQTT client and a broker) blocked in the
 * client's async `connect` while the broker's Rust thread called its Lua auth
 * callback: each module had its own callback queue, so the wait in one never ran
 * the other's callback, and both sides waited until the connect timed out.
 *
 * Here "module A" waits for a Rust future (uniffi_lua_block_on) that a Rust thread
 * completes only after a callback it posted to "module B" has run on the Lua thread.
 * Built as the Lua module `dispatcher` by scripts/ci/lua_e2e.nu. */
#define _POSIX_C_SOURCE 200809L /* alarm */
#include <pthread.h>
#include <unistd.h>

#include "uniffi_lua.h"

static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t polled_cond = PTHREAD_COND_INITIALIZER;
static bool polled = false;
static uniffi_continuation_fn continuation;
static uint64_t continuation_data;
static bool callback_ran = false;

/* A Rust future's poll: remembers the continuation, which the Rust thread calls. */
static void fake_poll(uint64_t future, uniffi_continuation_fn cont, uint64_t data) {
    (void)future;
    pthread_mutex_lock(&lock);
    continuation = cont;
    continuation_data = data;
    polled = true;
    pthread_cond_signal(&polled_cond);
    pthread_mutex_unlock(&lock);
}

static void callback(lua_State *L, void *arg) {
    (void)L;
    *(bool *)arg = true;
}

static void *rust_thread(void *raw) {
    /* Rust calls a callback of module B and waits for it... */
    uniffi_lua_invoke((uniffi_lua_dispatcher *)raw, callback, &callback_ran, false);
    /* ...then completes the future module A waits for. */
    pthread_mutex_lock(&lock);
    while (!polled) {
        pthread_cond_wait(&polled_cond, &lock);
    }
    uniffi_continuation_fn cont = continuation;
    uint64_t data = continuation_data;
    pthread_mutex_unlock(&lock);
    cont(data, UNIFFI_RUST_FUTURE_POLL_READY);
    return NULL;
}

static int run(lua_State *L) {
    uniffi_lua_dispatcher *a = uniffi_lua_shared_dispatcher(L);
    uniffi_lua_dispatcher *b = uniffi_lua_shared_dispatcher(L);
    if (a != b) {
        return luaL_error(L, "two modules of one Lua state got different dispatchers");
    }
    alarm(10); /* a deadlock fails the test instead of hanging it */
    pthread_t thread;
    if (pthread_create(&thread, NULL, rust_thread, b) != 0) {
        return luaL_error(L, "could not start a thread");
    }
    uniffi_lua_block_on(a, 1, fake_poll);
    pthread_join(thread, NULL);
    alarm(0);
    lua_pushboolean(L, callback_ran);
    return 1;
}

int luaopen_dispatcher(lua_State *L);

int luaopen_dispatcher(lua_State *L) {
    lua_newtable(L);
    lua_pushcfunction(L, run);
    lua_setfield(L, -2, "run");
    return 1;
}
