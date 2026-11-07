#import <AppKit/AppKit.h>
#import <CoreGraphics/CoreGraphics.h>

__attribute__((constructor)) static void start_probe(void) {
    fprintf(stderr, "CLICK_PROBE loaded pid=%d watchdog=65s\n", getpid());
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 65 * NSEC_PER_SEC), dispatch_get_global_queue(QOS_CLASS_UTILITY, 0), ^{
        fprintf(stderr, "CLICK_PROBE watchdog expired\n");
        _exit(124);
    });
    dispatch_async(dispatch_get_main_queue(), ^{
        __block int ticks = 0;
        __block BOOL failed = NO;
        [NSTimer scheduledTimerWithTimeInterval:0.1 repeats:YES block:^(NSTimer *timer) {
            ticks++;
            if (getenv("CLICK_PROBE_UNHIDE")) {
                if (ticks == 30) [NSApp hide:nil];
                if (ticks == 50) [NSApp unhide:nil];
            }
            CFArrayRef cg = CGWindowListCopyWindowInfo(kCGWindowListOptionOnScreenOnly, kCGNullWindowID);
            NSMutableSet *onScreen = [NSMutableSet set];
            for (NSDictionary *w in (__bridge NSArray *)cg) {
                [onScreen addObject:w[(__bridge NSString *)kCGWindowNumber]];
            }
            NSMutableArray *rows = [NSMutableArray array];
            for (NSWindow *w in NSApp.windows) {
                BOOL overlay = w.level > 0;
                BOOL unsafe = overlay && [onScreen containsObject:@(w.windowNumber)] && !w.ignoresMouseEvents && w.frame.size.width > 1000;
                if (unsafe) failed = YES;
                [rows addObject:@{@"id": @(w.windowNumber), @"title": w.title,
                    @"class": NSStringFromClass(w.class), @"onScreen": @([onScreen containsObject:@(w.windowNumber)]),
                    @"frame": NSStringFromRect(w.frame), @"level": @(w.level),
                    @"alpha": @(w.alphaValue), @"visible": @(w.isVisible),
                    @"ignoresMouse": @(w.ignoresMouseEvents), @"key": @(w.isKeyWindow),
                    @"main": @(w.isMainWindow), @"collection": @(w.collectionBehavior),
                    @"unsafe": @(unsafe)}];
            }
            if (ticks == 1 || ticks % 10 == 0 || failed) {
                NSData *json = [NSJSONSerialization dataWithJSONObject:rows options:0 error:nil];
                fprintf(stderr, "CLICK_PROBE %d active=%d hidden=%d modal=%ld %s\n", ticks, NSApp.active, NSApp.hidden, (long)NSApp.modalWindow.windowNumber, [[NSString alloc] initWithData:json encoding:NSUTF8StringEncoding].UTF8String);
                for (NSDictionary *w in (__bridge NSArray *)cg) {
                    if ([w[(__bridge NSString *)kCGWindowOwnerPID] intValue] == getpid()) {
                        fprintf(stderr, "CLICK_PROBE_CG %s\n", w.description.UTF8String);
                    }
                }
            }
            if (cg) CFRelease(cg);
            if (ticks >= 600 || failed) {
                fprintf(stderr, "CLICK_PROBE %s\n", failed ? "FAIL: full-screen idle overlay takes clicks" : "PASS: idle overlays never take clicks");
                [timer invalidate];
                if (failed) _exit(42);
                [NSApp terminate:nil];
            }
        }];
    });
}
