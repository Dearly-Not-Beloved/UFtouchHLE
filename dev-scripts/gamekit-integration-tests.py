#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
"""Real ARMv7 GameKit calls over independent emulator instances and loopback.
No game assets, patched guest code, fake matching peers, or Apple services.
Build cargo build --release first. Requires macOS ARMv7 compiler/linker.
"""
import importlib.util
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import shutil
import socket
import subprocess
import time

ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'target/gamekit-tests'
GUEST=r'''
#include "system_headers.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <pthread.h>
extern void *dispatch_semaphore_create(long);
extern long dispatch_semaphore_wait(void *, uint64_t);
@interface NSError:NSObject
- (NSInteger)code;
- (NSString *)domain;
@end
extern NSString *const GKErrorDomain;
@interface NSData (GameKitTests)
+ (id)dataWithBytes:(const void *)bytes length:(NSUInteger)length;
@end
@interface NSString (GameKitTests)
- (NSUInteger)length;
@end
@interface GKLocalPlayer:NSObject
+ (id)localPlayer;
- (NSString *)playerID;
- (BOOL)isAuthenticated;
- (id)authenticateHandler;
- (void)setAuthenticateHandler:(void (^)(id,NSError *))handler;
@end
@interface GKMatchRequest:NSObject
- (void)setMinPlayers:(NSUInteger)n; - (NSUInteger)minPlayers;
- (void)setMaxPlayers:(NSUInteger)n; - (NSUInteger)maxPlayers;
- (void)setDefaultNumberOfPlayers:(NSUInteger)n; - (NSUInteger)defaultNumberOfPlayers;
- (void)setPlayerGroup:(NSUInteger)n; - (NSUInteger)playerGroup;
- (void)setPlayerAttributes:(uint32_t)n; - (uint32_t)playerAttributes;
- (void)setPlayersToInvite:(id)array; - (id)playersToInvite;
@end
@interface GKMatch:NSObject
- (NSArray *)playerIDs;
- (NSUInteger)expectedPlayerCount;
- (void)setDelegate:(id)delegate; - (id)delegate;
- (void)disconnect;
- (BOOL)sendData:(NSData *)data toPlayers:(NSArray *)players withDataMode:(NSUInteger)mode error:(NSError **)error;
- (BOOL)sendDataToAllPlayers:(NSData *)data withDataMode:(NSUInteger)mode error:(NSError **)error;
@end
@interface GKMatchmaker:NSObject
+ (id)sharedMatchmaker;
- (void)findMatchForRequest:(GKMatchRequest *)r withCompletionHandler:(void (^)(GKMatch *,NSError *))block;
- (void)addPlayersToMatch:(GKMatch *)match matchRequest:(GKMatchRequest *)r completionHandler:(void (^)(NSError *))block;
- (void)finishMatchmakingForMatch:(GKMatch *)match;
- (void)cancel;
@end
#define CHECK(...) do { if(!(__VA_ARGS__)){ printf("FAIL line %d: %s\n",__LINE__,#__VA_ARGS__); fflush(stdout); exit(1); } } while(0)
static pthread_t main_thread;
static void *semaphore;
static int auth_calls, stale_auth, tokens_dead, completions, error_code;
static GKMatch *match;
static int host, stay, add_mode, received, udp_received, empty_received, connected, disconnected, failures, added, unsupported_add;
static void tick(void) { dispatch_semaphore_wait(semaphore,0); usleep(10000); }
#define WAIT(x) do {int remaining=1500; while(!(x) && remaining--)tick(); CHECK(x);}while(0)
static void mark(const char *s) {puts(s);fflush(stdout);}
@interface Token:NSObject @end
@implementation Token
- (void)dealloc {tokens_dead++;[super dealloc];}
@end
extern void *_Block_copy(const void *);
extern void _Block_release(const void *);
static void (^byref_writer)(void);
static int (^byref_reader)(void);
static Token *(^byref_object)(void);
static void prepare_byref(Token *token) {
    __block int value=7;
    __block Token *object=token;
    byref_writer=_Block_copy(^{value++;});
    byref_reader=_Block_copy(^{return value;});
    byref_object=_Block_copy(^{return object;});
    value=31;
    CHECK(byref_reader()==31);byref_writer();CHECK(value==32);
}
static void test_byref(void) {
    int before=tokens_dead;Token *token=[Token new];prepare_byref(token);
    CHECK(byref_reader()==32);byref_writer();CHECK(byref_reader()==33);
    CHECK(byref_object()==token);
    _Block_release(byref_writer);CHECK(byref_reader()==33);
    void *again=_Block_copy(byref_reader);CHECK(again==(void *)byref_reader);
    _Block_release(byref_reader);CHECK(((int(^)(void))again)()==33);
    _Block_release(again);_Block_release(byref_object);
    [token release];CHECK(tokens_dead==before+1);
    mark("BLOCK BYREF PASS");
}
@interface CALayer (LobbyKVC)
- (void)setValue:(id)value forKeyPath:(id)path;
- (void)setAffineTransform:(CGAffineTransform)t;
- (CGAffineTransform)affineTransform;
@end
static void test_layer_scale(void) {
    CALayer *layer=[CALayer new];CGAffineTransform t={0,2,-3,0,17,29};
    [layer setAffineTransform:t];NSString *path=[NSString stringWithUTF8String:"transform.scale"];
    [layer setValue:[NSNumber numberWithFloat:0.5f] forKeyPath:path];
    t=[layer affineTransform];CHECK(t.a==0 && t.b==0.5f && t.c==-0.5f && t.d==0 && t.tx==17 && t.ty==29);
    [layer setValue:[NSNumber numberWithFloat:0.25f] forKeyPath:path];
    t=[layer affineTransform];CHECK(t.b==0.25f && t.c==-0.25f && t.tx==17 && t.ty==29);
    [layer setValue:[NSNumber numberWithFloat:0.0f] forKeyPath:path];
    [layer setValue:[NSNumber numberWithFloat:1.0f] forKeyPath:path];
    t=[layer affineTransform];CHECK(t.a==1 && t.d==1 && t.b==0 && t.c==0 && t.tx==17 && t.ty==29);
    [layer release];mark("LAYER SCALE KVC PASS");
}
extern void *SCNetworkReachabilityCreateWithAddress(void *,const void *);
extern int SCNetworkReachabilityGetFlags(void *,unsigned *);
@interface NSURLConnection:NSObject
- (id)initWithRequest:(id)request delegate:(id)delegate startImmediately:(BOOL)start;
- (void)start;
- (void)cancel;
- (id)currentRequest;
@end
static int http_failed,http_dead;
@interface HTTPDelegate:NSObject @end
@implementation HTTPDelegate
- (void)connection:(id)connection didFailWithError:(NSError *)error {
    CHECK(pthread_self()==main_thread);CHECK(connection!=nil);
    CHECK([connection currentRequest]!=nil);CHECK([error code]==-1009);
    http_failed++;
}
- (void)dealloc {http_dead++;[super dealloc];}
@end
static void test_route_and_http_failure(void) {
    unsigned char address[16]={16,2};unsigned flags=0;
    void *route=SCNetworkReachabilityCreateWithAddress(NULL,address);
    CHECK(route!=NULL);int ready=0;
    for(int i=0;i<100 && !ready;i++){ready=SCNetworkReachabilityGetFlags(route,&flags);tick();}
    CHECK(ready && flags==2);CFRelease(route);
    id request=[NSObject new];HTTPDelegate *delegate=[HTTPDelegate new];
    NSURLConnection *connection=[[NSURLConnection alloc] initWithRequest:request delegate:delegate startImmediately:NO];
    CHECK(connection!=nil);tick();CHECK(http_failed==0);
    [connection start];[connection start];[delegate release];
    CHECK(http_failed==0 && http_dead==0);[connection release];
    WAIT(http_failed==1);CHECK(http_dead==1);
    delegate=[HTTPDelegate new];
    connection=[[NSURLConnection alloc] initWithRequest:request delegate:delegate startImmediately:YES];
    [connection cancel];[delegate release];[connection release];tick();
    CHECK(http_failed==1 && http_dead==2);[request release];
    mark("LOCAL ROUTE AND HTTP FAILURE PASS");
}
@interface Delegate:NSObject @end
@implementation Delegate
- (void)match:(GKMatch *)m player:(NSString *)p didChangeState:(NSUInteger)state {
    CHECK(pthread_self()==main_thread); CHECK([p length]==16);
    if(state==1)connected++; else if(state==2)disconnected++; else CHECK(0);
}
- (void)match:(GKMatch *)m didReceiveData:(NSData *)data fromPlayer:(NSString *)p {
    CHECK(pthread_self()==main_thread); CHECK([p length]==16);
    if([data length]==0) {
        if(host){CHECK([m sendDataToAllPlayers:data withDataMode:0 error:NULL]);}
        else {empty_received++;}return;
    }
    CHECK([data length]==4);
    const unsigned char *bytes=[data bytes];
    CHECK(bytes[2]==0 && bytes[3]==255);
    if(host) {
        NSError *error=(id)1;
        CHECK([m sendData:data toPlayers:[NSArray arrayWithObjects:p,nil] withDataMode:(bytes[0]==0xb1) error:&error]);
        CHECK(error==nil);
    } else if(bytes[0]==0xa0) {CHECK(bytes[1]==received); received++;}
    else {CHECK(bytes[0]==0xb1); udp_received++;}
}
- (void)match:(GKMatch *)m didFailWithError:(NSError *)error {
    CHECK(pthread_self()==main_thread);CHECK([error code]==3);failures++;
}
@end
static void *auth_worker(void *argument) {
    GKLocalPlayer *p=(id)argument;
    Token *token=[Token new];
    [p setAuthenticateHandler:^(id view,NSError *error){
        CHECK(pthread_self()==main_thread);CHECK(view==nil && error==nil);CHECK([p isAuthenticated]);
        CHECK([token class]!=nil);auth_calls++;
    }];
    [token release];return NULL;
}
static void authenticate(void) {
    int before=tokens_dead;
    GKLocalPlayer *p=[GKLocalPlayer localPlayer];CHECK(p!=nil && p==[GKLocalPlayer localPlayer]);
    CHECK([[p playerID] length]==16);CHECK(![p isAuthenticated]);
    [p setAuthenticateHandler:^(id view,NSError *error){stale_auth++;}];
    pthread_t worker;CHECK(pthread_create(&worker,NULL,auth_worker,p)==0);CHECK(pthread_join(worker,NULL)==0);
    CHECK(tokens_dead==before);CHECK([p authenticateHandler]!=nil);CHECK(auth_calls==0);
    WAIT(auth_calls==1);CHECK(stale_auth==0);
    [p setAuthenticateHandler:nil];CHECK([p authenticateHandler]==nil);CHECK(tokens_dead==before+1);
}
static GKMatchRequest *request(void) {
    GKMatchRequest *r=[GKMatchRequest new];
    [r setMinPlayers:2];[r setMaxPlayers:4];[r setDefaultNumberOfPlayers:2];
    [r setPlayerGroup:7];[r setPlayerAttributes:0x12];
    CHECK([r minPlayers]==2 && [r maxPlayers]==4 && [r defaultNumberOfPlayers]==2);
    CHECK([r playerGroup]==7 && [r playerAttributes]==0x12);
    [r setPlayersToInvite:[NSArray array]];CHECK([[r playersToInvite] count]==0);
    [r setPlayersToInvite:nil];CHECK([r playersToInvite]==nil);
    return r;
}
static void find(GKMatchRequest *r) {
    int before=tokens_dead;Token *token=[Token new];
    [[GKMatchmaker sharedMatchmaker] findMatchForRequest:r withCompletionHandler:^(GKMatch *m,NSError *error){
        CHECK([token class]!=nil);
        CHECK(pthread_self()==main_thread);completions++;
        if(error){CHECK(m==nil);error_code=[error code];CHECK([[error domain] isEqualToString:GKErrorDomain]);}
        else {CHECK(m!=nil && [m expectedPlayerCount]>=1);match=[m retain];}
    }];
    [token release];CHECK(tokens_dead==before);
}
int main(int argc,char **argv) {
    NSAutoreleasePool *pool=[NSAutoreleasePool new];main_thread=pthread_self();semaphore=dispatch_semaphore_create(0);
    test_byref();test_layer_scale();
    test_route_and_http_failure();
    const char *mode=argc>1?argv[1]:"errors";
    GKMatchmaker *maker=[GKMatchmaker sharedMatchmaker];CHECK(maker==[GKMatchmaker sharedMatchmaker]);
    if(strcmp(mode,"errors")==0) {
        GKMatchRequest *r=request(); find(r);CHECK(completions==0);WAIT(completions==1);CHECK(error_code==6);
        authenticate();
        [r setMinPlayers:1];find(r);WAIT(completions==2);CHECK(error_code==13);[r setMinPlayers:2];
        [r setPlayersToInvite:[NSArray arrayWithObjects:[NSString stringWithUTF8String:"invite"],nil]];
        find(r);WAIT(completions==3);CHECK(error_code==13);[r setPlayersToInvite:nil];
        find(r);[maker cancel];CHECK(completions==3);WAIT(completions==4);CHECK(error_code==2);
        find(r);[r release];WAIT(completions==5);CHECK(error_code==3 && match==nil);
        [maker cancel];mark("GAMEKIT ERRORS PASS");[pool drain];return 0;
    }
    authenticate();GKMatchRequest *r=request();
    if(strcmp(mode,"reject")==0){[r setPlayerGroup:8];find(r);WAIT(completions==1);CHECK(error_code==3 && match==nil);mark("GAMEKIT INCOMPATIBLE PASS");return 0;}
    host=strncmp(mode,"host",4)==0;add_mode=strcmp(mode,"host_add")==0 || strcmp(mode,"client_add")==0;stay=strcmp(mode,"stay")==0;
    [r setPlayerAttributes:(host?0xffff0000U:0x0000ffffU)];
    if(strcmp(mode,"host_badmask")==0 || strcmp(mode,"badmask")==0) {
        [r setPlayerAttributes:0xffff0000U];find(r);WAIT(completions==1);
        CHECK(error_code==3 && match==nil);[r release];mark("GAMEKIT MASK REJECT PASS");return 0;
    }
    find(r);CHECK(completions==0);mark("MATCH_REQUESTED");WAIT(completions==1);CHECK(error_code==0 && match!=nil);
    CHECK([match expectedPlayerCount]>=1 && [[match playerIDs] count]==0);Delegate *delegate=[Delegate new];[match setDelegate:delegate];CHECK([match delegate]==delegate);WAIT(connected>0);CHECK([[match playerIDs] count]>=1);
    [maker finishMatchmakingForMatch:match];
    if(add_mode) {
        [r setPlayerAttributes:0xffff0000U];
        [maker addPlayersToMatch:match matchRequest:r completionHandler:^(NSError *error){CHECK(error==nil);CHECK([[match playerIDs] count]==2);added++;}];
        CHECK(added==0 && [match expectedPlayerCount]==1);mark("ADD_REQUESTED");WAIT(added==1);CHECK([match expectedPlayerCount]==0);
        [maker finishMatchmakingForMatch:match];for(int i=0;i<100;i++)tick();
    } else if(host) {WAIT(disconnected>0);}
    else if(stay) {WAIT(failures>0);}
    else {
        [maker addPlayersToMatch:match matchRequest:r completionHandler:^(NSError *error){CHECK([error code]==2);unsupported_add++;}];
        CHECK(unsupported_add==0 && [match expectedPlayerCount]==1);[maker cancel];WAIT(unsupported_add==1);
        NSError *error=(id)1;unsigned char bytes[4]={0xa0,0,0,255};
        CHECK([match sendDataToAllPlayers:[NSData dataWithBytes:NULL length:0] withDataMode:0 error:&error]);WAIT(empty_received==1);
        for(int i=0;i<10;i++){bytes[1]=i;NSData *d=[NSData dataWithBytes:bytes length:4];CHECK([match sendDataToAllPlayers:d withDataMode:0 error:&error]);CHECK(error==nil);}
        WAIT(received==10);
        bytes[0]=0xb1;
        for(int i=0;i<100 && udp_received==0;i++){NSData *d=[NSData dataWithBytes:bytes length:4];CHECK([match sendData:d toPlayers:[match playerIDs] withDataMode:1 error:&error]);tick();}
        CHECK(udp_received>0);
        NSData *data=[NSData dataWithBytes:bytes length:4];
        CHECK(![match sendDataToAllPlayers:data withDataMode:2 error:&error] && [error code]==17);
        CHECK(![match sendDataToAllPlayers:data withDataMode:2 error:NULL]);
        CHECK(![match sendDataToAllPlayers:nil withDataMode:0 error:&error] && [error code]==17);
        NSString *unknown=[NSString stringWithUTF8String:"unknown"];
        CHECK(![match sendData:data toPlayers:[NSArray arrayWithObjects:unknown,nil] withDataMode:0 error:&error] && [error code]==17);
        unsigned char huge[1101]={0};data=[NSData dataWithBytes:huge length:1101];
        CHECK(![match sendDataToAllPlayers:data withDataMode:1 error:&error] && [error code]==17);
    }
    CHECK(connected>0);[match setDelegate:nil];CHECK([match delegate]==nil);[match disconnect];
    NSError *error=nil;NSData *empty=[NSData dataWithBytes:NULL length:0];
    CHECK(![match sendDataToAllPlayers:empty withDataMode:0 error:&error] && [error code]==3);
    CHECK([[match playerIDs] count]==0);[match release];[delegate release];[r release];
    mark(host?"GAMEKIT HOST PASS":"GAMEKIT CLIENT PASS");[pool drain];return 0;
}
'''

def main():
    os.chdir(ROOT);OUT.mkdir(parents=True,exist_ok=True)
    spec=importlib.util.spec_from_file_location('helper',ROOT/'dev-scripts/macos-integration-tests.py')
    h=importlib.util.module_from_spec(spec);spec.loader.exec_module(h)
    snapshot=OUT/'touchHLE-tested'
    shutil.copy2(h.TOUCHHLE,snapshot)
    h.TOUCHHLE=str(snapshot)
    sources=[*sorted((ROOT/'src/frameworks/game_kit').glob('*.rs')),ROOT/'src/transport/worker.rs',ROOT/'src/transport/mod.rs',ROOT/'src/libc/dispatch.rs',ROOT/'src/frameworks/foundation/ns_url_connection.rs',ROOT/'src/frameworks/system_configuration/sc_network_reachability.rs',ROOT/'src/options.rs',ROOT/'src/frameworks/core_animation/ca_layer.rs',Path(__file__).resolve()]
    manifest={'scope':'Synthetic ARM guest; loopback TCP/UDP, not real MHP2G',
        'binary':str(snapshot),'binary_sha256':hashlib.sha256(snapshot.read_bytes()).hexdigest(),
        'source_sha256':{str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sources}}
    h.OUT=str(OUT);h.STUBS=str(OUT/'stubs');links=h.make_stubs()
    (OUT/'guest.m').write_text(GUEST);(OUT/'start.s').write_text(h.START_S)
    app=OUT/'GameKit.app';app.mkdir(exist_ok=True)
    with (app/'Info.plist').open('wb') as file:
        plistlib.dump(dict(CFBundleExecutable='GameKit',CFBundleIdentifier='org.touchhle.tests.gamekit',CFBundleVersion='1.0',CFBundleName='GameKit'),file)
    h.run(['clang',*h.CFLAGS,'-c',str(OUT/'start.s'),'-o',str(OUT/'start.o')])
    h.run(['clang',*h.CFLAGS,*h.OBJC_FLAGS,'-fblocks','-I'+h.SOURCE,'-c',str(OUT/'guest.m'),'-o',str(OUT/'guest.o')])
    h.run(['clang',*h.CFLAGS,'-nostartfiles','-e','start',str(OUT/'start.o'),str(OUT/'guest.o'),*links,'-o',str(app/'GameKit')])
    children=[]
    def spawn(label,mode,role,number,timeout=5000,extra=()):
        path=OUT/(label+'.log');file=path.open('w')
        p=subprocess.Popen([h.TOUCHHLE,str(app),'--headless','--no-error-popup','--allow-network-access',
            role,*extra,'--gamekit-player-id='+f'{number:032x}','--gamekit-timeout-ms='+str(timeout),'--args',mode],stdout=file,stderr=subprocess.STDOUT)
        file.close();children.append(p);return p,path
    def wait_text(p,path,pattern,timeout=15):
        deadline=time.monotonic()+timeout
        while time.monotonic()<deadline:
            output=path.read_text(errors='replace')
            found=re.search(pattern,output)
            if found:return found
            if p.poll() is not None:raise AssertionError(output[-7000:])
            time.sleep(.03)
        raise AssertionError('timeout '+pattern+'\n'+path.read_text()[-7000:])
    def finish(p,path,marker):
        code=p.wait(timeout=20);output=path.read_text(errors='replace')
        assert code==0 and marker in output and 'panicked at' not in output,output[-7000:]
    try:
        p,path=spawn('errors','errors','--gamekit-host=127.0.0.1:0',10,300);finish(p,path,'GAMEKIT ERRORS PASS')
        print('PASS authentication/block ownership, request properties, async error/cancel/timeout')
        host,hlog=spawn('host','host','--gamekit-host=127.0.0.1:0',1)
        address=wait_text(host,hlog,r'\[LocalGameKit\] listening (127\.0\.0\.1:\d+)').group(1)
        bad,blog=spawn('incompatible','reject','--gamekit-join='+address,9);finish(bad,blog,'GAMEKIT INCOMPATIBLE PASS')
        client,clog=spawn('client','client','--gamekit-join='+address,2)
        finish(client,clog,'GAMEKIT CLIENT PASS');finish(host,hlog,'GAMEKIT HOST PASS')
        print('PASS real two-emulator TCP ordered echo, UDP echo, NSError, delegate and disconnect')
        host,hlog=spawn('host-badmask','host_badmask','--gamekit-host=127.0.0.1:0',6,800)
        address=wait_text(host,hlog,r'\[LocalGameKit\] listening (127\.0\.0\.1:\d+)').group(1)
        client,clog=spawn('client-badmask','badmask','--gamekit-join='+address,7,800)
        finish(host,hlog,'GAMEKIT MASK REJECT PASS');finish(client,clog,'GAMEKIT MASK REJECT PASS')
        print('PASS noncomplementary actual peers never produce a successful match')
        host,hlog=spawn('host-add','host_add','--gamekit-host=127.0.0.1:0',3)
        address=wait_text(host,hlog,r'\[LocalGameKit\] listening (127\.0\.0\.1:\d+)').group(1)
        a,alog=spawn('second','stay','--gamekit-join='+address,4)
        wait_text(host,hlog,'ADD_REQUESTED')
        b,blog=spawn('third','stay','--gamekit-join='+address,5)
        finish(host,hlog,'GAMEKIT HOST PASS');finish(a,alog,'GAMEKIT CLIENT PASS');finish(b,blog,'GAMEKIT CLIENT PASS')
        print('PASS addPlayers actual third peer, one-argument completion, host loss and object cleanup')
        host,hlog=spawn('host-client-add','host','--gamekit-host=127.0.0.1:0',3)
        address=wait_text(host,hlog,r'\[LocalGameKit\] listening (127\.0\.0\.1:\d+)').group(1)
        a,alog=spawn('client-add','client_add','--gamekit-join='+address,4)
        wait_text(a,alog,'ADD_REQUESTED')
        b,blog=spawn('client-add-third','stay','--gamekit-join='+address,5)
        finish(a,alog,'GAMEKIT CLIENT PASS');finish(host,hlog,'GAMEKIT HOST PASS');finish(b,blog,'GAMEKIT CLIENT PASS')
        print('PASS Client requests admission and changes attributes; actual third peer completes addPlayers')
        with socket.socket(socket.AF_INET,socket.SOCK_DGRAM) as reservation:
            reservation.bind(('127.0.0.1',0));discovery_port=reservation.getsockname()[1]
        host,hlog=spawn('discovered-host','host','--gamekit-host=127.0.0.1:0',6,
            extra=('--gamekit-room=Synthetic ARM Room','--gamekit-broadcast=127.0.0.1:'+str(discovery_port)))
        wait_text(host,hlog,r'\[LocalGameKit\] listening (127\.0\.0\.1:\d+)')
        client,clog=spawn('discovered-client','client','--gamekit-discover=auto',7,
            extra=('--gamekit-discovery-bind=127.0.0.1:'+str(discovery_port),))
        finish(client,clog,'GAMEKIT CLIENT PASS');finish(host,hlog,'GAMEKIT HOST PASS')
        print('PASS GameKit room discovery selects actual session and performs TCP/UDP echo')
        print('GAMEKIT ARM INTEGRATION PASS; no real P2G or physical-device verification')
        (OUT/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
    finally:
        for p in children:
            if p.poll() is None:p.kill()
            p.wait(timeout=3)

if __name__=='__main__':main()
