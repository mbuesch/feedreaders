// -*- coding: utf-8 -*-
// Copyright (C) 2024-2025 Michael Büsch <m@bues.ch>
// SPDX-License-Identifier: GPL-2.0-or-later

let feed_update_rev_request = null;

function send_feed_update_rev_request() {
    feed_update_rev_request = new XMLHttpRequest();
    feed_update_rev_request.open('GET', '/cgi-bin/feeds/feed_update_rev');

    feed_update_rev_request.onreadystatechange = function() {
        if (feed_update_rev_request.readyState == 4) { // Done
            var elem_rev_static = document.getElementById("feed_update_revision_static");
            var elem_rev_dynamic = document.getElementById("feed_update_revision_dynamic");
            var elem_feed_list_th = document.getElementById("feed_list_th");
            var elem_feed_list_th_a = document.getElementById("feed_list_th_a");

            if (elem_rev_static && elem_rev_dynamic && elem_feed_list_th && elem_feed_list_th_a) {
                var again = true;

                if (feed_update_rev_request.status == 200) { // Ok
                    elem_rev_dynamic.textContent = feed_update_rev_request.responseText;
                    var feed_update_rev_dynamic = parseInt(elem_rev_dynamic.textContent);
                    var feed_update_rev_static = parseInt(elem_rev_static.textContent);

                    if (feed_update_rev_dynamic != feed_update_rev_static) {
                        elem_feed_list_th_a.textContent += " (UPDATES AVAILABLE)";
                        elem_feed_list_th.style.color = "red";
                        elem_feed_list_th.style.fontWeight = "bold";
                        again = false;
                    }
                }

                if (again) {
                    setTimeout(send_feed_update_rev_request, 10000);
                }
            }
        }
    };

    feed_update_rev_request.send(null);
}

send_feed_update_rev_request();

// vim: ts=4 sw=4 expandtab
