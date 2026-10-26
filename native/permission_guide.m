#import <AppKit/AppKit.h>
#import <AVFoundation/AVFoundation.h>
#import <ApplicationServices/ApplicationServices.h>
#import <CoreGraphics/CoreGraphics.h>
#import <CoreImage/CoreImage.h>
#import <IOKit/hidsystem/IOHIDLib.h>
#import <QuartzCore/QuartzCore.h>
#import <dlfcn.h>

typedef NS_ENUM(NSInteger, HexPermission) {
    HexPermissionInputMonitoring = 0,
    HexPermissionAccessibility = 1,
    HexPermissionMicrophone = 2,
    HexPermissionSystemAudio = 3,
};

static NSPanel *guidePanel;
static NSTimer *guideTimer;
static BOOL sawSettingsWindow;
static HexPermission activePermission;
static NSView *guideTile;

/// The tile flies on its own from the click in Hex's window to its place in
/// the guide, so the eye can follow it to System Settings.
static NSPanel *flightPanel;
static CALayer *flightLayer;
static NSTimer *flightTimer;
static NSPoint flightOrigin;
static CFTimeInterval flightShownAt;
static CFTimeInterval flightStartedAt;
static const CGFloat flightPadding = 40;
static const CFTimeInterval flightLift = 0.18;
static const CFTimeInterval flightDuration = 0.62;
/// Settings that has not opened by then gets no flight.
static const CFTimeInterval flightPatience = 4;

@interface HexDragView : NSView <NSDraggingSource>
@property(nonatomic, strong) NSURL *bundleURL;
@property(nonatomic, strong) NSImage *appIcon;
@property(nonatomic, copy) NSString *appName;
@property(nonatomic, strong) NSEvent *mouseDownEvent;
@end

/// The name System Settings lists the app under.
static NSString *hexAppName(NSURL *bundleURL) {
    NSString *name = [[NSFileManager defaultManager] displayNameAtPath:bundleURL.path];
    if ([name.pathExtension.lowercaseString isEqualToString:@"app"]) {
        name = name.stringByDeletingPathExtension;
    }
    return name.length > 0 ? name : @"Hex";
}

/// macOS 27 renamed the Accessibility list.
static BOOL accessibilityRenamed(void) {
    NSOperatingSystemVersion version = {27, 0, 0};
    return [NSProcessInfo.processInfo isOperatingSystemAtLeastVersion:version];
}

/// The name of the Privacy & Security list each permission lives in.
const char *hex_permission_list_name(int permission) {
    switch ((HexPermission)permission) {
        case HexPermissionInputMonitoring:
            return "Input Monitoring";
        case HexPermissionAccessibility:
            return accessibilityRenamed() ? "Device Control and Data Access" : "Accessibility";
        case HexPermissionMicrophone:
            return "Microphone";
        case HexPermissionSystemAudio:
            return "System Audio Recording Only";
    }
    return "Privacy & Security";
}

static BOOL isDark(NSAppearance *appearance) {
    NSAppearanceName match = [appearance bestMatchFromAppearancesWithNames:@[
        NSAppearanceNameAqua,
        NSAppearanceNameDarkAqua,
    ]];
    return [match isEqualToString:NSAppearanceNameDarkAqua];
}

/// The app's tile as System Settings will list it: icon and name.
static void drawTile(NSRect bounds, NSImage *icon, NSString *name, BOOL dark) {
    NSBezierPath *background = [NSBezierPath bezierPathWithRoundedRect:bounds
                                                               xRadius:8
                                                               yRadius:8];
    [[NSColor colorWithWhite:(dark ? 0.18 : 0.89) alpha:1] setFill];
    [background fill];
    [[NSColor colorWithWhite:(dark ? 1 : 0) alpha:0.10] setStroke];
    background.lineWidth = 1;
    [background stroke];

    [icon drawInRect:NSMakeRect(10, 6, 32, 32)];
    NSDictionary *attributes = @{
        NSFontAttributeName: [NSFont systemFontOfSize:13 weight:NSFontWeightSemibold],
        NSForegroundColorAttributeName: [NSColor colorWithWhite:(dark ? 0.94 : 0.12) alpha:1],
    };
    [name drawAtPoint:NSMakePoint(52, 14) withAttributes:attributes];
}

@implementation HexDragView

- (instancetype)initWithFrame:(NSRect)frame bundleURL:(NSURL *)bundleURL {
    self = [super initWithFrame:frame];
    if (self) {
        _bundleURL = bundleURL;
        _appIcon = [[NSWorkspace sharedWorkspace] iconForFile:bundleURL.path];
        _appIcon.size = NSMakeSize(32, 32);
        _appName = hexAppName(bundleURL);
    }
    return self;
}

- (BOOL)acceptsFirstMouse:(NSEvent *)event {
    return YES;
}

- (void)resetCursorRects {
    [self addCursorRect:self.bounds cursor:NSCursor.openHandCursor];
}

- (void)drawRect:(NSRect)dirtyRect {
    [super drawRect:dirtyRect];
    drawTile(self.bounds, self.appIcon, self.appName, isDark(self.effectiveAppearance));
}

- (void)mouseDown:(NSEvent *)event {
    self.mouseDownEvent = event;
}

- (void)mouseDragged:(NSEvent *)event {
    if (self.mouseDownEvent == nil) {
        return;
    }
    CGFloat dx = event.locationInWindow.x - self.mouseDownEvent.locationInWindow.x;
    CGFloat dy = event.locationInWindow.y - self.mouseDownEvent.locationInWindow.y;
    if (hypot(dx, dy) < 3) {
        return;
    }

    NSDraggingItem *item = [[NSDraggingItem alloc] initWithPasteboardWriter:self.bundleURL];
    NSBitmapImageRep *representation = [self bitmapImageRepForCachingDisplayInRect:self.bounds];
    [self cacheDisplayInRect:self.bounds toBitmapImageRep:representation];
    NSImage *snapshot = [[NSImage alloc] initWithSize:self.bounds.size];
    [snapshot addRepresentation:representation];
    [item setDraggingFrame:self.bounds contents:snapshot];

    NSDraggingSession *session = [self beginDraggingSessionWithItems:@[ item ]
                                                               event:self.mouseDownEvent
                                                              source:self];
    session.animatesToStartingPositionsOnCancelOrFail = YES;
    session.draggingFormation = NSDraggingFormationNone;
    self.mouseDownEvent = nil;
}

- (NSDragOperation)draggingSession:(NSDraggingSession *)session
        sourceOperationMaskForDraggingContext:(NSDraggingContext)context {
    return NSDragOperationCopy;
}

@end

static CGRect currentSettingsFrame(void) {
    CFArrayRef windowInfo = CGWindowListCopyWindowInfo(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        kCGNullWindowID
    );
    NSArray *windows = CFBridgingRelease(windowInfo);
    CGRect best = CGRectNull;
    for (NSDictionary *window in windows) {
        NSString *owner = window[(id)kCGWindowOwnerName];
        if (![owner isEqualToString:@"System Settings"] &&
            ![owner isEqualToString:@"System Preferences"]) {
            continue;
        }
        if ([window[(id)kCGWindowLayer] integerValue] != 0) {
            continue;
        }
        CGRect frame;
        if (!CGRectMakeWithDictionaryRepresentation(
                (__bridge CFDictionaryRef)window[(id)kCGWindowBounds], &frame)) {
            continue;
        }
        if (frame.size.width < 300 || frame.size.height < 200) {
            continue;
        }
        if (CGRectIsNull(best) || frame.size.width * frame.size.height >
                                      best.size.width * best.size.height) {
            best = frame;
        }
    }
    return best;
}

static NSRect appKitFrame(CGRect frame) {
    NSScreen *primary = NSScreen.screens.firstObject;
    if (primary == nil) {
        return NSRectFromCGRect(frame);
    }
    return NSMakeRect(
        frame.origin.x,
        primary.frame.size.height - frame.origin.y - frame.size.height,
        frame.size.width,
        frame.size.height
    );
}

/// System Audio Recording has no public query; TCC's preflight answers it
/// (0 is granted). Without it, the guide waits for System Settings to close.
static BOOL systemAudioGranted(void) {
    static int (*preflight)(CFStringRef, CFDictionaryRef);
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        void *handle = dlopen(
            "/System/Library/PrivateFrameworks/TCC.framework/Versions/A/TCC", RTLD_NOW);
        if (handle != NULL) {
            preflight = dlsym(handle, "TCCAccessPreflight");
        }
    });
    return preflight != NULL && preflight(CFSTR("kTCCServiceAudioCapture"), NULL) == 0;
}

static BOOL permissionGranted(void) {
    switch (activePermission) {
        // What System Settings shows now: the preflights keep their launch
        // answer until Hex restarts.
        case HexPermissionInputMonitoring:
            return IOHIDCheckAccess(kIOHIDRequestTypeListenEvent) == kIOHIDAccessTypeGranted;
        case HexPermissionAccessibility:
            return AXIsProcessTrusted();
        case HexPermissionMicrophone:
            return [AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio] ==
                   AVAuthorizationStatusAuthorized;
        case HexPermissionSystemAudio:
            return systemAudioGranted();
    }
    return NO;
}

static void endFlight(void) {
    [flightTimer invalidate];
    flightTimer = nil;
    [flightPanel orderOut:nil];
    flightPanel = nil;
    flightLayer = nil;
    guideTile.hidden = NO;
    guidePanel.alphaValue = 1;
}

static void closeGuide(void) {
    endFlight();
    [guideTimer invalidate];
    guideTimer = nil;
    [guidePanel orderOut:nil];
    guidePanel = nil;
    guideTile = nil;
    sawSettingsWindow = NO;
}

static CGFloat clamp01(CGFloat value) {
    return MIN(MAX(value, 0), 1);
}

static CGFloat easeInOutCubic(CGFloat t) {
    return t < 0.5 ? 4 * t * t * t : 1 - pow(-2 * t + 2, 3) / 2;
}

static CGFloat easeOutCubic(CGFloat t) {
    return 1 - pow(1 - t, 3);
}

/// Where the tile lands: its slot in the guide, in screen coordinates.
static NSPoint tileTarget(void) {
    NSRect slot = [guideTile convertRect:guideTile.bounds toView:nil];
    NSRect screen = [guidePanel convertRectToScreen:slot];
    return NSMakePoint(NSMidX(screen), NSMidY(screen));
}

static void placeFlight(NSPoint center, CGFloat scale, CGFloat blur, CGFloat opacity) {
    NSSize size = flightPanel.frame.size;
    [flightPanel setFrameOrigin:NSMakePoint(center.x - size.width / 2, center.y - size.height / 2)];
    [CATransaction begin];
    [CATransaction setDisableActions:YES];
    flightLayer.transform = CATransform3DMakeScale(scale, scale, 1);
    flightLayer.opacity = (float)opacity;
    [flightLayer setValue:@(blur) forKeyPath:@"filters.blur.inputRadius"];
    [CATransaction commit];
}

static void stepFlight(void) {
    CFTimeInterval now = CACurrentMediaTime();
    if (flightStartedAt == 0) {
        // Lifted out of the row, waiting for System Settings to open.
        if (now - flightShownAt > flightPatience) {
            endFlight();
            return;
        }
        CGFloat t = easeOutCubic(clamp01((now - flightShownAt) / flightLift));
        placeFlight(NSMakePoint(flightOrigin.x, flightOrigin.y + 12 * t), 0.7 + 0.1 * t,
                    4 * (1 - t), t);
        return;
    }
    CGFloat t = clamp01((now - flightStartedAt) / flightDuration);
    CGFloat e = easeInOutCubic(t);
    NSPoint start = NSMakePoint(flightOrigin.x, flightOrigin.y + 12);
    NSPoint end = tileTarget();
    CGFloat distance = hypot(end.x - start.x, end.y - start.y);
    NSPoint control = NSMakePoint((start.x + end.x) / 2,
                                  MAX(start.y, end.y) + MIN(160, distance * 0.25));
    CGFloat u = 1 - e;
    NSPoint point = NSMakePoint(u * u * start.x + 2 * u * e * control.x + e * e * end.x,
                                u * u * start.y + 2 * u * e * control.y + e * e * end.y);
    placeFlight(point, 0.8 + 0.2 * e, 6 * sin(M_PI * e), 1);
    guidePanel.alphaValue = easeOutCubic(clamp01((t - 0.45) / 0.55));
    if (t >= 1) {
        endFlight();
    }
}

/// The click that asked for the guide, when it was in one of Hex's windows.
static BOOL clickInHexWindow(NSPoint point) {
    for (NSWindow *window in NSApp.windows) {
        if (window.visible && window != guidePanel && window != flightPanel &&
            NSPointInRect(point, window.frame)) {
            return YES;
        }
    }
    return NO;
}

/// Lifts a copy of the tile out of Hex's window at `origin`.
static void beginFlight(NSPoint origin, NSURL *bundleURL, NSSize tileSize) {
    flightOrigin = origin;
    flightShownAt = CACurrentMediaTime();
    flightStartedAt = 0;
    NSImage *icon = [[NSWorkspace sharedWorkspace] iconForFile:bundleURL.path];
    icon.size = NSMakeSize(32, 32);
    NSString *name = hexAppName(bundleURL);
    BOOL dark = isDark(NSApp.effectiveAppearance);
    NSImage *image = [NSImage imageWithSize:tileSize
                                    flipped:NO
                             drawingHandler:^BOOL(NSRect rect) {
        drawTile(rect, icon, name, dark);
        return YES;
    }];

    NSSize size = NSMakeSize(tileSize.width + 2 * flightPadding, tileSize.height + 2 * flightPadding);
    flightPanel = [[NSPanel alloc] initWithContentRect:NSMakeRect(0, 0, size.width, size.height)
                                             styleMask:NSWindowStyleMaskBorderless |
                                                       NSWindowStyleMaskNonactivatingPanel
                                               backing:NSBackingStoreBuffered
                                                 defer:NO];
    flightPanel.level = NSFloatingWindowLevel + 1;
    flightPanel.collectionBehavior = NSWindowCollectionBehaviorCanJoinAllSpaces |
                                     NSWindowCollectionBehaviorFullScreenAuxiliary;
    flightPanel.opaque = NO;
    flightPanel.backgroundColor = NSColor.clearColor;
    flightPanel.hasShadow = NO;
    flightPanel.ignoresMouseEvents = YES;
    flightPanel.hidesOnDeactivate = NO;
    flightPanel.releasedWhenClosed = NO;

    NSView *host = [[NSView alloc] initWithFrame:NSMakeRect(0, 0, size.width, size.height)];
    host.layer = [CALayer layer];
    host.wantsLayer = YES;
    host.layerUsesCoreImageFilters = YES;
    flightLayer = [CALayer layer];
    flightLayer.bounds = CGRectMake(0, 0, tileSize.width, tileSize.height);
    flightLayer.position = CGPointMake(size.width / 2, size.height / 2);
    flightLayer.contents = image;
    flightLayer.contentsScale = NSScreen.mainScreen.backingScaleFactor;
    flightLayer.shadowOpacity = 0.25;
    flightLayer.shadowRadius = 12;
    flightLayer.shadowOffset = CGSizeMake(0, -6);
    CIFilter *blur = [CIFilter filterWithName:@"CIGaussianBlur"];
    blur.name = @"blur";
    [blur setValue:@0 forKey:kCIInputRadiusKey];
    flightLayer.filters = @[ blur ];
    [host.layer addSublayer:flightLayer];
    flightPanel.contentView = host;

    placeFlight(origin, 0.7, 4, 0);
    [flightPanel orderFrontRegardless];
    flightTimer = [NSTimer timerWithTimeInterval:1.0 / 120 repeats:YES block:^(NSTimer *timer) {
        (void)timer;
        stepFlight();
    }];
    [[NSRunLoop mainRunLoop] addTimer:flightTimer forMode:NSRunLoopCommonModes];
}

static void updateGuidePosition(void) {
    if (permissionGranted()) {
        closeGuide();
        return;
    }
    CGRect quartzFrame = currentSettingsFrame();
    if (CGRectIsNull(quartzFrame)) {
        if (sawSettingsWindow) {
            closeGuide();
        }
        return;
    }
    sawSettingsWindow = YES;
    NSRect settings = appKitFrame(quartzFrame);
    NSSize size = guidePanel.frame.size;
    NSScreen *screen = nil;
    for (NSScreen *candidate in NSScreen.screens) {
        if (NSIntersectsRect(candidate.frame, settings)) {
            screen = candidate;
            break;
        }
    }
    if (screen == nil) {
        screen = NSScreen.mainScreen;
    }
    NSRect visible = screen != nil ? screen.visibleFrame : settings;
    CGFloat x = NSMaxX(settings) - size.width - 16;
    CGFloat y = NSMinY(settings) + 16;
    x = MIN(MAX(x, NSMinX(visible) + 8), NSMaxX(visible) - size.width - 8);
    y = MIN(MAX(y, NSMinY(visible) + 8), NSMaxY(visible) - size.height - 8);
    [guidePanel setFrameOrigin:NSMakePoint(x, y)];
    if (flightPanel != nil && flightStartedAt == 0) {
        flightStartedAt = CACurrentMediaTime();
    }
    [guidePanel orderFrontRegardless];
}

void hex_show_permission_guide(int permission) {
    NSCAssert(NSThread.isMainThread, @"permission guide must run on the main thread");
    NSPoint click = NSEvent.mouseLocation;
    BOOL fly = clickInHexWindow(click) &&
               !NSWorkspace.sharedWorkspace.accessibilityDisplayShouldReduceMotion;
    closeGuide();
    activePermission = (HexPermission)permission;

    NSURL *bundleURL = NSBundle.mainBundle.bundleURL.standardizedURL;
    NSString *listName = [NSString stringWithUTF8String:hex_permission_list_name(permission)];
    NSString *appName = hexAppName(bundleURL);
    NSString *pane;
    switch (activePermission) {
        case HexPermissionInputMonitoring:
            pane = @"Privacy_ListenEvent";
            break;
        case HexPermissionAccessibility:
            pane = @"Privacy_Accessibility";
            break;
        case HexPermissionMicrophone:
            pane = @"Privacy_Microphone";
            break;
        case HexPermissionSystemAudio:
            pane = @"Privacy_AudioCapture";
            break;
    }
    // The Microphone list only holds apps that asked, so a refused
    // microphone is switched on rather than dragged in.
    BOOL draggable = activePermission != HexPermissionMicrophone;
    NSURL *settingsURL = [NSURL URLWithString:[NSString stringWithFormat:
        @"x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?%@", pane]];
    [[NSWorkspace sharedWorkspace] openURL:settingsURL];
    // A development binary has no bundle to list, so it gets the pane alone.
    if (![bundleURL.pathExtension.lowercaseString isEqualToString:@"app"]) {
        return;
    }

    NSSize panelSize = NSMakeSize(480, draggable ? 116 : 60);
    guidePanel = [[NSPanel alloc]
        initWithContentRect:NSMakeRect(0, 0, panelSize.width, panelSize.height)
                  styleMask:NSWindowStyleMaskBorderless |
                            NSWindowStyleMaskNonactivatingPanel |
                            NSWindowStyleMaskFullSizeContentView
                    backing:NSBackingStoreBuffered
                      defer:NO];
    guidePanel.floatingPanel = YES;
    guidePanel.level = NSFloatingWindowLevel;
    guidePanel.collectionBehavior = NSWindowCollectionBehaviorCanJoinAllSpaces |
                                    NSWindowCollectionBehaviorFullScreenAuxiliary;
    guidePanel.opaque = NO;
    guidePanel.backgroundColor = NSColor.clearColor;
    guidePanel.hasShadow = YES;
    guidePanel.hidesOnDeactivate = NO;
    guidePanel.releasedWhenClosed = NO;

    NSView *content = [[NSView alloc] initWithFrame:NSMakeRect(0, 0, panelSize.width, panelSize.height)];
    content.wantsLayer = YES;
    content.layer.backgroundColor = NSColor.controlBackgroundColor.CGColor;
    content.layer.cornerRadius = 20;
    content.layer.borderWidth = 1;
    content.layer.borderColor = [NSColor colorWithWhite:0 alpha:0.10].CGColor;

    NSString *text = draggable
        ? [NSString stringWithFormat:@"↑  Drag %@ into the %@ list above", appName, listName]
        : [NSString stringWithFormat:@"↑  Switch on %@ in the %@ list above", appName, listName];
    NSTextField *instruction = [NSTextField labelWithString:text];
    instruction.frame = NSMakeRect(18, draggable ? 73 : 18, panelSize.width - 36, 24);
    instruction.font = [NSFont systemFontOfSize:13 weight:NSFontWeightMedium];
    instruction.textColor = NSColor.labelColor;
    [content addSubview:instruction];

    if (draggable) {
        HexDragView *dragView = [[HexDragView alloc]
            initWithFrame:NSMakeRect(18, 17, panelSize.width - 36, 44)
                 bundleURL:bundleURL];
        [content addSubview:dragView];
        guideTile = dragView;
    }
    guidePanel.contentView = content;
    if (draggable && fly) {
        guideTile.hidden = YES;
        guidePanel.alphaValue = 0;
        beginFlight(click, bundleURL, guideTile.frame.size);
    }

    guideTimer = [NSTimer scheduledTimerWithTimeInterval:0.12
                                                 repeats:YES
                                                   block:^(NSTimer *timer) {
        (void)timer;
        updateGuidePosition();
    }];
    updateGuidePosition();
}
