// A disposable AppKit receiver. No event tap, AX grants or input injection.
// Build with clang -fobjc-arc -framework AppKit Receiver.m -o Receiver.
#import <AppKit/AppKit.h>
#import <sys/sysctl.h>

static NSString *const root = @"/Users/admin/lab";
static NSTextView *other;

static void record(NSString *label) {
    NSString *path = [root stringByAppendingPathComponent:@"received.log"];
    FILE *file = fopen(path.fileSystemRepresentation, "a");
    if (!file) abort();
    fprintf(file, "%s\n", label.UTF8String);
    fclose(file);
}

@interface Receiver : NSTextView
@end
@implementation Receiver
- (BOOL)validateMenuItem:(NSMenuItem *)item {
    // This fixture explicitly accepts PNG, even when a plain NSTextView's
    // built-in Paste would be disabled for a clipboard with no text.
    if (item.action == @selector(paste:)) return YES;
    return [super validateMenuItem:item];
}
- (void)paste:(id)sender {
    NSPasteboard *board = NSPasteboard.generalPasteboard;
    NSString *value = [board stringForType:NSPasteboardTypeString];
    BOOL png = [board dataForType:NSPasteboardTypePNG] != nil;
    NSString *label = png ? @"Image attached" : [value containsString:@"Move the caption below the chart."] ? @"Move the caption below the chart." : value;
    if (!label) label = @"Unexpected format";
    record(label);
    [self insertText:[label stringByAppendingString:@"\n"] replacementRange:self.selectedRange];
    if (!png) return;
    NSString *mode = [NSString stringWithContentsOfFile:[root stringByAppendingPathComponent:@"mode"] encoding:NSUTF8StringEncoding error:nil];
    if ([mode isEqual:@"change-focus"]) [self.window makeFirstResponder:other];
    if ([mode isEqual:@"external-copy"]) {
        [board clearContents];
        [board setString:@"https://example.org/new-copy" forType:NSPasteboardTypeString];
    }
}
- (void)insertNewline:(id)sender { record(@"Unexpected Return"); [super insertNewline:sender]; }
@end

int main(void) { @autoreleasepool {
    int guest = 0; size_t size = sizeof guest;
    if (sysctlbyname("kern.hv_vmm_present", &guest, &size, NULL, 0) || !guest ||
        ![[NSFileManager defaultManager] fileExistsAtPath:[root stringByAppendingPathComponent:@"silence.wav"]]) return 2;
    NSApplication *app = NSApplication.sharedApplication;
    [app setActivationPolicy:NSApplicationActivationPolicyRegular];
    NSMenu *bar = [NSMenu new], *edit = [[NSMenu alloc] initWithTitle:@"Edit"];
    [bar addItemWithTitle:@"Receiver" action:NULL keyEquivalent:@""].submenu = [NSMenu new];
    [bar addItemWithTitle:@"Edit" action:NULL keyEquivalent:@""].submenu = edit;
    [edit addItemWithTitle:@"Paste" action:@selector(paste:) keyEquivalent:@"v"];
    app.mainMenu = bar;
    NSWindow *window = [[NSWindow alloc] initWithContentRect:NSMakeRect(0,0,700,460) styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable backing:NSBackingStoreBuffered defer:NO];
    window.title = @"Paste Last — sample document";
    Receiver *notes = [[Receiver alloc] initWithFrame:NSMakeRect(20,100,660,320)];
    notes.font = [NSFont systemFontOfSize:20]; notes.string = @"Design review\n\n";
    notes.selectedRange = NSMakeRange(notes.string.length,0);
    other = [[Receiver alloc] initWithFrame:NSMakeRect(20,15,660,60)];
    [window.contentView addSubview:other]; [window.contentView addSubview:notes];
    [window center]; [window makeKeyAndOrderFront:nil]; [window makeFirstResponder:notes];
    [app activateIgnoringOtherApps:YES]; [app run];
} }
