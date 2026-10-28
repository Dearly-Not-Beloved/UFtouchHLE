#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
"""macOS ARMv7 diagnostic regression: trace on/off preserve existing behavior.

No game assets, remote sockets, synthetic GameKit success, or production mocks.
Uses the current real GameKit/Reachability/HTTP stubs and real dispatch blocks.
Build with cargo build --release, then run this script from any directory.
"""

import importlib.util
import json
import os
from pathlib import Path
import plistlib
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]

GUEST = r'''
#include "system_headers.h"
#include <arpa/inet.h>
#include <errno.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
// Match the public SCNetworkReachability ABI without importing the SDK's
// umbrella CoreFoundation header, which conflicts with the test SDK geometry.
typedef const void *SCNetworkReachabilityRef;
typedef uint32_t SCNetworkReachabilityFlags;
extern SCNetworkReachabilityRef SCNetworkReachabilityCreateWithAddress(CFAllocatorRef, const struct sockaddr *);
extern Boolean SCNetworkReachabilitySetCallback(SCNetworkReachabilityRef, void (*)(SCNetworkReachabilityRef, SCNetworkReachabilityFlags, void *), void *);
extern Boolean SCNetworkReachabilityGetFlags(SCNetworkReachabilityRef, SCNetworkReachabilityFlags *);
typedef void *dispatch_queue_t;
extern dispatch_queue_t dispatch_get_global_queue(long, unsigned long);
extern void dispatch_async(dispatch_queue_t, void (^)(void));
extern void *dispatch_semaphore_create(long);
extern long dispatch_semaphore_wait(void *, uint64_t);
@interface GKLocalPlayer : NSObject
+ (id)localPlayer;
- (BOOL)isAuthenticated;
- (void)setAuthenticateHandler:(void (^)(id,id))handler;
@end
@interface GKMatchRequest : NSObject
- (void)setMinPlayers:(NSUInteger)n;
- (NSUInteger)minPlayers;
- (void)setMaxPlayers:(NSUInteger)n;
- (NSUInteger)maxPlayers;
- (void)setPlayerGroup:(NSUInteger)n;
- (NSUInteger)playerGroup;
@end
@interface GKMatchmaker : NSObject
+ (id)sharedMatchmaker;
- (void)findMatchForRequest:(id)request withCompletionHandler:(void (^)(id,id))handler;
@end
@interface GKSession : NSObject
- (id)initWithSessionID:(id)s displayName:(id)n sessionMode:(NSInteger)mode;
- (void)setAvailable:(BOOL)b;
- (BOOL)isAvailable;
- (BOOL)sendDataToAllPeers:(id)data withDataMode:(NSInteger)mode error:(id *)error;
@end
@interface NSURLConnection : NSObject
+ (id)sendSynchronousRequest:(id)request returningResponse:(id *)response error:(id *)error;
@end
static int completions, callbacks, delegate_calls, reachability_calls;
static void changed(SCNetworkReachabilityRef r, SCNetworkReachabilityFlags f, void *c) {
    (void)r; (void)f; (void)c; reachability_calls++;
}
@interface GameCenterSupport : NSObject
- (void)enqueueDiagnosticBlocks;
- (void)match:(id)match didReceiveData:(id)data fromPlayer:(id)player;
@end
@implementation GameCenterSupport
- (void)enqueueDiagnosticBlocks {
    void (^block)(void) = ^{ callbacks++; };
    dispatch_async(dispatch_get_global_queue(0,0), block);
    dispatch_async(dispatch_get_global_queue(0,0), block);
}
- (void)match:(id)m didReceiveData:(id)d fromPlayer:(id)p {
    (void)m; (void)d; (void)p; delegate_calls++;
}
@end
#define CHECK(x) do { if (!(x)) { printf("FAIL line %d errno %d\n", __LINE__, errno); return 1; } } while (0)
int main(int argc, char **argv) {
    NSAutoreleasePool *pool = [NSAutoreleasePool new];
    BOOL online = argc > 1 && strcmp(argv[1], "online") == 0;
    id player = [GKLocalPlayer localPlayer];
    CHECK(player == nil && ![player isAuthenticated]);
    [player setAuthenticateHandler:^(id v,id e){ (void)v; (void)e; completions++; }];
    GKMatchRequest *request = [GKMatchRequest new];
    [request setMinPlayers:2]; [request setMaxPlayers:4]; [request setPlayerGroup:777];
    CHECK([request minPlayers] == 2 && [request maxPlayers] == 4 && [request playerGroup] == 777);
    if (argc > 1 && strcmp(argv[1], "missing") == 0) {
        [request performSelector:@selector(networkTraceIntentionallyMissing)];
        return 9;
    }
    [[GKMatchmaker sharedMatchmaker] findMatchForRequest:request withCompletionHandler:^(id m,id e){
        (void)m; (void)e; completions++;
    }];
    GKSession *session = [[GKSession alloc] initWithSessionID:nil displayName:nil sessionMode:2];
    [session setAvailable:YES]; CHECK([session isAvailable]);
    id error = (id)1;
    CHECK(![session sendDataToAllPeers:nil withDataMode:0 error:&error] && error == nil);
    struct sockaddr_in addr = {0}; addr.sin_len=sizeof(addr); addr.sin_family=AF_INET;
    addr.sin_addr.s_addr=inet_addr("169.254.0.0");
    SCNetworkReachabilityRef reach = SCNetworkReachabilityCreateWithAddress(NULL,(struct sockaddr *)&addr);
    CHECK(reach != NULL);
    errno = 777;
    CHECK(!SCNetworkReachabilitySetCallback(reach, changed, NULL) && errno == 777);
    SCNetworkReachabilityFlags flags = 0xabcdef;
    BOOL got = SCNetworkReachabilityGetFlags(reach, &flags);
    CHECK(got == online && errno == 777);
    CHECK(flags == (online ? ((1U<<1) | (1U<<17)) : 0xabcdef));
    if (!online) { CHECK(socket(AF_INET, SOCK_DGRAM, 0) == -1 && errno == EPROTONOSUPPORT); }
    id response = (id)1; error = nil;
    // Exercise the existing nil-request stub without constructing a URL or
    // contacting any service. It must still write its original NSError.
    id connection = [NSURLConnection sendSynchronousRequest:nil
                                        returningResponse:&response error:&error];
    CHECK(connection == nil && response == nil && error != nil);
    GameCenterSupport *driver = [GameCenterSupport new];
    [driver enqueueDiagnosticBlocks];
    [driver match:nil didReceiveData:nil fromPlayer:nil];
    CHECK(callbacks == 0 && completions == 0);
    // Existing semaphore wait drains pending main-queue blocks even headless.
    // Use a zero timeout so this fixture never needs a UI event loop.
    CHECK(dispatch_semaphore_wait(dispatch_semaphore_create(0), 0) != 0);
    CHECK(callbacks == 2 && delegate_calls == 1 && completions == 0 && reachability_calls == 0);
    CFRelease(reach); [driver release]; [session release]; [request release];
    printf("WITNESS online=%d flags=%u callbacks=%d delegates=%d completions=%d reachability=%d\n",
           online,flags,callbacks,delegate_calls,completions,reachability_calls);
    [pool drain]; puts("NETWORK TRACE GUEST PASS"); return 0;
}
'''


def parse_records(output):
    return [json.loads(line.split("[NetworkTrace] ", 1)[1])
            for line in output.splitlines() if "[NetworkTrace] " in line]


def main():
    if sys.platform != "darwin":
        sys.exit("This fixture requires macOS's ARMv7 compiler/linker.")
    os.chdir(ROOT)
    spec = importlib.util.spec_from_file_location("integration", ROOT / "dev-scripts/macos-integration-tests.py")
    helper = importlib.util.module_from_spec(spec); spec.loader.exec_module(helper)
    out = ROOT / "target/network-trace-smoke"
    out.mkdir(parents=True, exist_ok=True)
    helper.OUT, helper.STUBS = str(out), str(out / "stubs")
    links = helper.make_stubs()
    (out / "guest.m").write_text(GUEST)
    (out / "start.s").write_text(helper.START_S)
    app = out / "NetworkTrace.app"; app.mkdir(exist_ok=True)
    with (app / "Info.plist").open("wb") as file:
        plistlib.dump(dict(CFBundleExecutable="NetworkTrace",CFBundleName="NetworkTrace",
                          CFBundleIdentifier="org.touchhle.tests.networktrace",CFBundleVersion="1.0"),file)
    helper.run(["clang",*helper.CFLAGS,"-c",str(out / "start.s"),"-o",str(out / "start.o")])
    helper.run(["clang",*helper.CFLAGS,*helper.OBJC_FLAGS,"-fblocks","-I"+helper.SOURCE,
                "-c",str(out / "guest.m"),"-o",str(out / "guest.o")])
    helper.run(["clang",*helper.CFLAGS,"-nostartfiles","-e","start",str(out / "start.o"),
                str(out / "guest.o"),*links,"-o",str(app / "NetworkTrace")])

    def run(label, options, argument="offline", success=True):
        result = subprocess.run([helper.TOUCHHLE,str(app),"--headless","--no-error-popup",
                                 *options,"--args",argument],capture_output=True,text=True,timeout=30)
        output = result.stdout + result.stderr
        (out / (label + ".log")).write_text(output)
        if success and (result.returncode or "NETWORK TRACE GUEST PASS" not in output):
            raise AssertionError(label + "\n" + output[-6000:])
        if success:
            assert "panicked at" not in output, output[-6000:]
        if not success:
            assert result.returncode != 0 and 'does not respond to selector "networkTraceIntentionallyMissing"' in output, output[-3000:]
        records = parse_records(output)
        assert [r["seq"] for r in records] == list(range(1,len(records)+1))
        return output, records

    for mode in ("offline","online"):
        flags = ["--allow-network-access"] if mode == "online" else []
        off, silent = run(mode+"-off", flags, mode)
        on, records = run(mode+"-on", flags+["--network-trace"], mode)
        assert not silent and records
        witness = lambda output: [l for l in output.splitlines() if l.startswith("WITNESS ")]
        assert witness(off) == witness(on)
        events = {r["event"] for r in records}
        assert {"api_invoked","api_returned","object_created","state_written",
                "observed_failure","network_error","callback_not_scheduled",
                "callback_scheduled","callback_invoked","callback_returned"} <= events
        assert any(r['event']=='callback_invoked' and r['details'].get('context',{}).get('correlation')=='ambiguous_block_reuse' for r in records)
        assert any(r['event']=='network_error' and r['details'].get('code')==-1009 for r in records)
        reach = [r['details'] for r in records if r['event']=='reachability_result']
        assert len(reach)==1 and reach[0]['success']==(mode=='online')
        assert reach[0]['flags_written']==(mode=='online')
        assert any(r['event']=='network_error' and r['details'].get('api')=='_SCNetworkReachabilitySetCallback'
                   and r['details'].get('errno_if_initialized')==777 for r in records)
        calls = {r['seq'] for r in records if r['event'] in ('api_invoked','callback_invoked')}
        assert all(r['details']['call'] in calls for r in records
                   if r['event'] in ('api_returned','callback_returned'))
        assert all(r['details']['args_r0_r3'] is None for r in records
                   if r['event']=='callback_invoked' and r['details']['api']=='dispatch Block invocation')
        print("PASS trace on/off preserves",mode,"results, errno, state and callbacks")
    _, limited = run("limit",["--network-trace","--network-trace-limit=4"])
    assert len(limited) == 5 and limited[-1]["event"] == "trace_truncated"
    _, disabled = run("explicit-off",["--network-trace","--network-trace=off"])
    assert not disabled
    run("missing-off",[],"missing",False)
    _, records = run("missing-on",["--network-trace"],"missing",False)
    assert any(r['event']=='dispatch_error' for r in records)
    print("PASS trace limit, explicit disable and unchanged missing-selector failure")
    print("NETWORK TRACE SMOKE PASS (synthetic ARM guest; no P2G online capture)")


if __name__ == "__main__":
    main()
