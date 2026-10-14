/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 *
 * Parts of this file are derived from SDL 2's Android project template, which
 * has a different license. Please see vendor/SDL/LICENSE.txt for details.
 */
package org.touchhle.android;

import android.os.Build;
import android.os.Bundle;
import android.os.SystemClock;
import android.view.WindowManager;
import android.widget.Toast;

import org.libsdl.app.SDLActivity;

/**
 * A wrapper class over SDLActivity
 */

public class MainActivity extends SDLActivity {
    @Override
    protected void onCreate(Bundle savedInstanceState) {
        // Use the whole screen, including the area beside a display cutout
        // (e.g. a front camera). Otherwise Android leaves that area empty in
        // landscape and the app content isn't centered on the screen.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            WindowManager.LayoutParams attrs = getWindow().getAttributes();
            attrs.layoutInDisplayCutoutMode =
                WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES;
            getWindow().setAttributes(attrs);
        }
        super.onCreate(savedInstanceState);
    }

    // By default, the back button/gesture finishes the activity, which makes
    // touchHLE exit and any unsaved progress in the app is lost. Treat it like
    // the home button instead: touchHLE goes into the background (where the
    // app is paused) and can be resumed. It can still be closed from the
    // recent apps screen.
    //
    // With gesture navigation, swiping in from the side of the screen is
    // "back", which is easy to do by accident while playing. So back has to be
    // pressed twice in quick succession.
    private static final long BACK_CONFIRM_MS = 2000;
    private long lastBackPressTime = 0;

    @Override
    public void onBackPressed() {
        long now = SystemClock.uptimeMillis();
        if (now - lastBackPressTime < BACK_CONFIRM_MS) {
            lastBackPressTime = 0;
            moveTaskToBack(true);
        } else {
            lastBackPressTime = now;
            Toast.makeText(this, "再按一次返回键退到后台", Toast.LENGTH_SHORT).show();
        }
    }

    @Override
    public void superOnBackPressed() {
        onBackPressed();
    }

    @Override
    protected String[] getLibraries() {
        return new String[]{
            "SDL2",
            "touchHLE"
        };
    }
}
