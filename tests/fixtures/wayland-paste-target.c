#include <gtk/gtk.h>

static void changed(GtkTextBuffer *buffer, gpointer path) {
    GtkTextIter start, end;
    gtk_text_buffer_get_bounds(buffer, &start, &end);
    gchar *text = gtk_text_buffer_get_text(buffer, &start, &end, FALSE);
    g_file_set_contents(path, text, -1, NULL);
    g_free(text);
}

static gboolean focused(GtkWidget *widget, GdkEventFocus *event, gpointer path) {
    gchar *ready = g_strconcat(path, ".ready", NULL);
    g_file_set_contents(ready, "ready", -1, NULL);
    g_free(ready);
    return FALSE;
}

int main(int argc, char **argv) {
    gtk_init(&argc, &argv);
    if (argc != 2) return 2;
    GtkWidget *window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
    gtk_window_set_title(GTK_WINDOW(window), "Hex Wayland Test");
    gtk_window_set_default_size(GTK_WINDOW(window), 640, 320);
    GtkWidget *view = gtk_text_view_new();
    gtk_container_add(GTK_CONTAINER(window), view);
    g_signal_connect(gtk_text_view_get_buffer(GTK_TEXT_VIEW(view)), "changed", G_CALLBACK(changed), argv[1]);
    g_signal_connect(window, "destroy", G_CALLBACK(gtk_main_quit), NULL);
    g_signal_connect(window, "focus-in-event", G_CALLBACK(focused), argv[1]);
    gtk_widget_show_all(window);
    gtk_widget_grab_focus(view);
    gtk_main();
    return 0;
}
