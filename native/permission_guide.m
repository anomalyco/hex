#import <AppKit/AppKit.h>
#import <ApplicationServices/ApplicationServices.h>
#import <CoreGraphics/CoreGraphics.h>
#import <QuartzCore/QuartzCore.h>

typedef NS_ENUM(NSInteger, HexPermission) {
    HexPermissionInputMonitoring = 0,
    HexPermissionAccessibility = 1,
};

static NSPanel *guidePanel;
static NSTimer *guideTimer;
static BOOL sawSettingsWindow;
static HexPermission activePermission;

@interface HexDragView : NSView <NSDraggingSource>
@property(nonatomic, strong) NSURL *bundleURL;
@property(nonatomic, strong) NSImage *appIcon;
@property(nonatomic, strong) NSEvent *mouseDownEvent;
@end

@implementation HexDragView

- (instancetype)initWithFrame:(NSRect)frame bundleURL:(NSURL *)bundleURL {
    self = [super initWithFrame:frame];
    if (self) {
        _bundleURL = bundleURL;
        _appIcon = [[NSWorkspace sharedWorkspace] iconForFile:bundleURL.path];
        _appIcon.size = NSMakeSize(32, 32);
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
    NSAppearanceName appearance = [self.effectiveAppearance bestMatchFromAppearancesWithNames:@[
        NSAppearanceNameAqua,
        NSAppearanceNameDarkAqua,
    ]];
    BOOL dark = [appearance isEqualToString:NSAppearanceNameDarkAqua];
    NSBezierPath *background = [NSBezierPath bezierPathWithRoundedRect:self.bounds
                                                               xRadius:8
                                                               yRadius:8];
    [[NSColor colorWithWhite:(dark ? 0.18 : 0.89) alpha:1] setFill];
    [background fill];
    [[NSColor colorWithWhite:(dark ? 1 : 0) alpha:0.10] setStroke];
    background.lineWidth = 1;
    [background stroke];

    [self.appIcon drawInRect:NSMakeRect(10, 6, 32, 32)];
    NSDictionary *attributes = @{
        NSFontAttributeName: [NSFont systemFontOfSize:13 weight:NSFontWeightSemibold],
        NSForegroundColorAttributeName: [NSColor colorWithWhite:(dark ? 0.94 : 0.12) alpha:1],
    };
    [@"HEX" drawAtPoint:NSMakePoint(52, 14) withAttributes:attributes];
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

static BOOL permissionGranted(void) {
    switch (activePermission) {
        case HexPermissionInputMonitoring:
            return CGPreflightListenEventAccess();
        case HexPermissionAccessibility:
            return CGPreflightPostEventAccess();
    }
}

static void closeGuide(void) {
    [guideTimer invalidate];
    guideTimer = nil;
    [guidePanel orderOut:nil];
    guidePanel = nil;
    sawSettingsWindow = NO;
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
    NSScreen *screen = [NSScreen.screens filteredArrayUsingPredicate:
        [NSPredicate predicateWithBlock:^BOOL(NSScreen *candidate, NSDictionary *bindings) {
            (void)bindings;
            return NSIntersectsRect(candidate.frame, settings);
        }]].firstObject ?: NSScreen.mainScreen;
    NSRect visible = screen != nil ? screen.visibleFrame : settings;
    CGFloat x = NSMaxX(settings) - size.width - 16;
    CGFloat y = NSMinY(settings) + 16;
    x = MIN(MAX(x, NSMinX(visible) + 8), NSMaxX(visible) - size.width - 8);
    y = MIN(MAX(y, NSMinY(visible) + 8), NSMaxY(visible) - size.height - 8);
    [guidePanel setFrameOrigin:NSMakePoint(x, y)];
    [guidePanel orderFrontRegardless];
}

void hex_show_permission_guide(int permission) {
    NSCAssert(NSThread.isMainThread, @"permission guide must run on the main thread");
    closeGuide();
    activePermission = (HexPermission)permission;

    NSURL *bundleURL = NSBundle.mainBundle.bundleURL.standardizedURL;
    if (![bundleURL.pathExtension.lowercaseString isEqualToString:@"app"]) {
        return;
    }

    NSString *permissionName = activePermission == HexPermissionInputMonitoring
        ? @"Input Monitoring"
        : @"Accessibility";
    NSString *pane = activePermission == HexPermissionInputMonitoring
        ? @"Privacy_ListenEvent"
        : @"Privacy_Accessibility";

    NSSize panelSize = NSMakeSize(480, 116);
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

    NSTextField *instruction = [NSTextField labelWithString:[NSString stringWithFormat:
        @"↑  Drag HEX into the %@ list above", permissionName]];
    instruction.frame = NSMakeRect(18, 73, panelSize.width - 36, 24);
    instruction.font = [NSFont systemFontOfSize:13 weight:NSFontWeightMedium];
    instruction.textColor = NSColor.labelColor;
    [content addSubview:instruction];

    HexDragView *dragView = [[HexDragView alloc]
        initWithFrame:NSMakeRect(18, 17, panelSize.width - 36, 44)
             bundleURL:bundleURL];
    [content addSubview:dragView];
    guidePanel.contentView = content;

    NSURL *settingsURL = [NSURL URLWithString:[NSString stringWithFormat:
        @"x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?%@", pane]];
    [[NSWorkspace sharedWorkspace] openURL:settingsURL];
    guideTimer = [NSTimer scheduledTimerWithTimeInterval:0.12
                                                 repeats:YES
                                                   block:^(NSTimer *timer) {
        (void)timer;
        updateGuidePosition();
    }];
    updateGuidePosition();
}
