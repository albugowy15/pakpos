/* Native GTK diagnostic; build/run instructions are in the memory audit. */
#include <gtk/gtk.h>
#include <stdio.h>

static void
finalized(gpointer data, GObject *object)
{
    (void)object;
    *(gboolean *)data = TRUE;
}

int
main(void)
{
    gboolean disposed[2] = { FALSE, FALSE };

    gtk_init();
    for (int select = 0; select <= 1; select++) {
        GtkTextBuffer *buffer = gtk_text_buffer_new(NULL);
        g_object_weak_ref(G_OBJECT(buffer), finalized, &disposed[select]);
        gtk_text_buffer_set_text(buffer, "alpha beta alpha", -1);
        if (select) {
            GtkTextIter start, end;
            gtk_text_buffer_get_iter_at_offset(buffer, &start, 0);
            gtk_text_buffer_get_iter_at_offset(buffer, &end, 5);
            gtk_text_buffer_select_range(buffer, &start, &end);
            gtk_text_buffer_place_cursor(buffer, &start);
        }
        g_object_unref(buffer);
        printf("selected=%d finalized=%d\n", select, disposed[select]);
    }
    return 0;
}
