/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
package org.touchhle.android;

import android.app.Activity;
import android.app.AlertDialog;
import android.app.Dialog;
import android.content.ActivityNotFoundException;
import android.content.ClipData;
import android.content.Intent;
import android.content.SharedPreferences;
import android.content.res.ColorStateList;
import android.content.res.Configuration;
import android.database.Cursor;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.ColorFilter;
import android.graphics.Paint;
import android.graphics.Path;
import android.graphics.PixelFormat;
import android.graphics.Rect;
import android.graphics.Typeface;
import android.graphics.drawable.ColorDrawable;
import android.graphics.drawable.Drawable;
import android.graphics.drawable.GradientDrawable;
import android.graphics.drawable.RippleDrawable;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.LocaleList;
import android.provider.OpenableColumns;
import android.text.TextUtils;
import android.util.TypedValue;
import android.view.DisplayCutout;
import android.view.Gravity;
import android.view.View;
import android.view.ViewGroup;
import android.view.WindowManager;
import android.widget.BaseAdapter;
import android.widget.FrameLayout;
import android.widget.GridView;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.Switch;
import android.widget.TextView;
import android.widget.Toast;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.io.RandomAccessFile;
import java.net.HttpURLConnection;
import java.net.URL;
import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.text.SimpleDateFormat;
import java.util.Date;
import java.util.Locale;
import java.util.zip.ZipEntry;
import java.util.zip.ZipInputStream;
import java.util.zip.ZipOutputStream;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import org.json.JSONArray;
import org.json.JSONObject;

/**
 * touchHLE's launcher on Android: lists the apps, imports new ones, and lets
 * the quick options be changed, then starts MainActivity (touchHLE proper,
 * which runs in its own process) with the chosen app.
 *
 * This replaces the app picker (src/environment/app_picker.rs) on Android.
 * That one is an emulated iPhone OS app, so it's drawn at 320×480 and scaled
 * up, which makes it blurry and leaves black bars on modern screens. This
 * activity is drawn at the screen's real resolution and fills any shape of
 * screen, in either orientation.
 */
public class LauncherActivity extends Activity {
    static {
        System.loadLibrary("SDL2");
        System.loadLibrary("touchHLE");
    }

    /** See src/android_launcher.rs. */
    private static native byte[] nativeScanApps(String appsDir, String iconsDir, String languages);
    private static native byte[] nativeCopyrightText(android.content.res.AssetManager assets);
    private static native byte[] nativeVersion();

    private static final String APPS_DIR = "touchHLE_apps";
    private static final String LOG_FILE = "touchHLE_log.txt";
    private static final String[] WALLPAPER_FILES = {
        "touchHLE_wallpaper.png", "touchHLE_wallpaper.jpg", "touchHLE_wallpaper.jpeg",
    };
    private static final int REQUEST_IMPORT = 1;
    private static final int REQUEST_IMPORT_SAVE = 2;
    private static final int REQUEST_EXPORT_SAVE = 3;
    /// Where touchHLE keeps each app's data (see src/paths.rs), in a folder
    /// named after the app's bundle identifier.
    private static final String SANDBOX_DIR = "touchHLE_sandbox";
    /// Saves are backed up here before importing others over them.
    private static final String SAVE_BACKUPS_DIR = "touchHLE_save_backups";
    /// Exported saves go here if the system can't ask where to save them.
    private static final String SAVE_EXPORTS_DIR = "touchHLE_save_exports";
    /// Added to exported saves, to tell which app they're for.
    private static final String SAVE_MARKER = "touchHLE_save.txt";

    /// The newest release of this fork, checked for updates.
    private static final String LATEST_RELEASE_URL =
        "https://api.github.com/repos/wesleyicode/touchHLE/releases/latest";
    private static final String PREFS = "launcher";
    private static final String PREF_CHECK_UPDATES = "checkUpdates";
    private static final String PREF_IGNORED_VERSION = "ignoredUpdateVersion";

    private static final int TEXT = Color.WHITE;
    private static final int TEXT_DIM = 0xB3FFFFFF;
    private static final int ACCENT = 0xFF3D8BFF;
    private static final int WARNING = 0xFFFFC766;

    private static class App {
        String path;
        String name;
        Bitmap icon;
        /// Empty if unknown.
        String bundleId;
    }

    /// The app whose saves are being imported or exported (kept across
    /// activity recreation, since the system file picker is in between).
    private String saveBundleId;
    private String saveAppName;

    private final List<App> apps = new ArrayList<>();
    private boolean loading = true;
    private QuickOptions quickOptions;
    private Bitmap wallpaper;

    private AppAdapter adapter;
    private TextView statusText;
    /// Holds the "new version" banner, filled in once the check finishes.
    private FrameLayout updateSlot;
    /// The newer release found by [checkForUpdate], if any.
    private UpdateInfo update;

    private static class UpdateInfo {
        String version;
        String notes;
        /// The Android APK, or the release page if there's none.
        String downloadUrl;
    }
    private View importBigButton;

    // ---- Lifecycle ----

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        Intent intent = getIntent();
        if (!isTaskRoot() && intent != null && intent.hasCategory(Intent.CATEGORY_LAUNCHER)
            && Intent.ACTION_MAIN.equals(intent.getAction())) {
            // Opened from the home screen while an app is running on top of
            // the launcher: just go back to it.
            finish();
            return;
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            WindowManager.LayoutParams attrs = getWindow().getAttributes();
            attrs.layoutInDisplayCutoutMode =
                WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES;
            getWindow().setAttributes(attrs);
        }
        getAppsDir().mkdirs();
        if (savedInstanceState != null) {
            saveBundleId = savedInstanceState.getString("saveBundleId");
            saveAppName = savedInstanceState.getString("saveAppName");
        }
        quickOptions = QuickOptions.load(getBaseDir());
        wallpaper = loadWallpaper();
        buildUi();
        rescan();
        checkForUpdate();
    }

    @Override
    protected void onSaveInstanceState(Bundle outState) {
        super.onSaveInstanceState(outState);
        outState.putString("saveBundleId", saveBundleId);
        outState.putString("saveAppName", saveAppName);
    }

    @Override
    protected void onResume() {
        super.onResume();
        hideSystemBars();
        // touchHLE may have just exited (e.g. after a crash, whose log should
        // be pointed out) or files may have been changed elsewhere.
        if (!loading) {
            buildUi();
            rescan();
        }
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (hasFocus) {
            hideSystemBars();
        }
    }

    @Override
    public void onConfigurationChanged(Configuration newConfig) {
        super.onConfigurationChanged(newConfig);
        // The layout differs between portrait and landscape.
        buildUi();
    }

    @SuppressWarnings("deprecation")
    private void hideSystemBars() {
        try {
            getWindow().getDecorView().setSystemUiVisibility(
                View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                | View.SYSTEM_UI_FLAG_FULLSCREEN
                | View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY);
        } catch (Throwable e) {
            // Not essential.
        }
    }

    // ---- Files ----

    private File getBaseDir() {
        // Same as SDL_AndroidGetExternalStoragePath(), which touchHLE uses.
        File dir = getExternalFilesDir(null);
        return dir != null ? dir : getFilesDir();
    }

    private File getAppsDir() {
        return new File(getBaseDir(), APPS_DIR);
    }

    private static String languages() {
        StringBuilder result = new StringBuilder();
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
            LocaleList locales = LocaleList.getDefault();
            for (int i = 0; i < locales.size(); i++) {
                if (result.length() > 0) {
                    result.append(',');
                }
                result.append(locales.get(i).getLanguage());
            }
        } else {
            result.append(Locale.getDefault().getLanguage());
        }
        return result.toString();
    }

    private void rescan() {
        loading = true;
        updateStatus();
        final File iconsDir = new File(getCacheDir(), "launcher_icons");
        new Thread(() -> {
            File[] old = iconsDir.listFiles();
            if (old != null) {
                for (File file : old) {
                    file.delete();
                }
            }
            String result = new String(
                nativeScanApps(getAppsDir().getPath(), iconsDir.getPath(), languages()),
                StandardCharsets.UTF_8);
            final List<App> found = new ArrayList<>();
            for (String line : result.split("\n")) {
                String[] fields = line.split("\t", -1);
                if (fields.length < 5) {
                    continue;
                }
                App app = new App();
                app.path = fields[0];
                app.name = fields[1];
                app.icon = loadIcon(fields[2], fields[3], fields[4]);
                app.bundleId = fields.length > 5 ? fields[5] : "";
                found.add(app);
            }
            runOnUiThread(() -> {
                apps.clear();
                apps.addAll(found);
                loading = false;
                if (adapter != null) {
                    adapter.notifyDataSetChanged();
                }
                updateStatus();
            });
        }).start();
    }

    private static Bitmap loadIcon(String path, String width, String height) {
        if (path.isEmpty()) {
            return null;
        }
        try {
            int w = Integer.parseInt(width);
            int h = Integer.parseInt(height);
            byte[] pixels = readFile(new File(path), Integer.MAX_VALUE);
            if (pixels == null || pixels.length != w * h * 4) {
                return null;
            }
            Bitmap bitmap = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888);
            bitmap.copyPixelsFromBuffer(ByteBuffer.wrap(pixels));
            return bitmap;
        } catch (Exception e) {
            return null;
        }
    }

    /** Reads (at most the last {@code maxBytes} of) a file. */
    private static byte[] readFile(File file, int maxBytes) {
        try (RandomAccessFile f = new RandomAccessFile(file, "r")) {
            long length = f.length();
            int count = (int) Math.min(length, maxBytes);
            byte[] bytes = new byte[count];
            f.seek(length - count);
            f.readFully(bytes);
            return bytes;
        } catch (Exception e) {
            return null;
        }
    }

    private Bitmap loadWallpaper() {
        for (String name : WALLPAPER_FILES) {
            File file = new File(getBaseDir(), name);
            if (!file.isFile()) {
                continue;
            }
            try {
                BitmapFactory.Options options = new BitmapFactory.Options();
                options.inJustDecodeBounds = true;
                BitmapFactory.decodeFile(file.getPath(), options);
                int screen = Math.max(
                    getResources().getDisplayMetrics().widthPixels,
                    getResources().getDisplayMetrics().heightPixels);
                int sample = 1;
                while (Math.max(options.outWidth, options.outHeight) / (sample * 2) >= screen) {
                    sample *= 2;
                }
                options = new BitmapFactory.Options();
                options.inSampleSize = sample;
                return BitmapFactory.decodeFile(file.getPath(), options);
            } catch (Throwable e) {
                return null;
            }
        }
        return null;
    }

    /** Whether the last run of touchHLE crashed, judging by its log. */
    private boolean lastRunCrashed() {
        byte[] tail = readFile(new File(getBaseDir(), LOG_FILE), 64 * 1024);
        if (tail == null) {
            return false;
        }
        String log = new String(tail, StandardCharsets.UTF_8);
        // Written by the panic hook (see src/lib.rs and src/environment.rs).
        return log.contains("Panic at ") || log.contains("Panic: ")
            || log.contains("Register state immediately after panic");
    }

    // ---- UI ----

    private int dp(float value) {
        return Math.round(TypedValue.applyDimension(
            TypedValue.COMPLEX_UNIT_DIP, value, getResources().getDisplayMetrics()));
    }

    private boolean isLandscape() {
        return getResources().getConfiguration().orientation
            == Configuration.ORIENTATION_LANDSCAPE;
    }

    private Drawable buttonBackground(int color, float radius) {
        GradientDrawable shape = new GradientDrawable();
        shape.setColor(color);
        shape.setCornerRadius(radius);
        return new RippleDrawable(ColorStateList.valueOf(0x40FFFFFF), shape, null);
    }

    private TextView makeButton(String title, boolean primary, View.OnClickListener onClick) {
        TextView button = new TextView(this);
        button.setText(title);
        button.setTextColor(TEXT);
        button.setTextSize(TypedValue.COMPLEX_UNIT_SP, 15);
        button.setTypeface(Typeface.DEFAULT_BOLD);
        button.setGravity(Gravity.CENTER);
        button.setSingleLine(true);
        button.setPadding(dp(18), 0, dp(18), 0);
        button.setMinHeight(dp(44));
        button.setBackground(buttonBackground(primary ? ACCENT : 0x2EFFFFFF, dp(22)));
        button.setOnClickListener(onClick);
        return button;
    }

    private TextView makeText(String text, float sizeSp, int color, boolean bold) {
        TextView view = new TextView(this);
        view.setText(text);
        view.setTextSize(TypedValue.COMPLEX_UNIT_SP, sizeSp);
        view.setTextColor(color);
        if (bold) {
            view.setTypeface(Typeface.DEFAULT_BOLD);
        }
        return view;
    }

    private void buildUi() {
        FrameLayout root = new FrameLayout(this);
        if (wallpaper != null) {
            ImageView background = new ImageView(this);
            background.setImageBitmap(wallpaper);
            background.setScaleType(ImageView.ScaleType.CENTER_CROP);
            root.addView(background, new FrameLayout.LayoutParams(-1, -1));
            View shade = new View(this);
            shade.setBackgroundColor(0x66000000);
            root.addView(shade, new FrameLayout.LayoutParams(-1, -1));
        } else {
            root.setBackground(new GradientDrawable(
                GradientDrawable.Orientation.TL_BR,
                new int[]{0xFF27306B, 0xFF1A3F6E, 0xFF0F6A85}));
        }

        LinearLayout content = new LinearLayout(this);
        content.setOrientation(LinearLayout.VERTICAL);
        root.addView(content, new FrameLayout.LayoutParams(-1, -1));
        final int pad = dp(isLandscape() ? 20 : 16);
        content.setPadding(pad, pad, pad, pad);
        // Keep clear of display cutouts (e.g. the front camera).
        content.setOnApplyWindowInsetsListener((view, insets) -> {
            int left = 0, top = 0, right = 0, bottom = 0;
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                DisplayCutout cutout = insets.getDisplayCutout();
                if (cutout != null) {
                    left = cutout.getSafeInsetLeft();
                    top = cutout.getSafeInsetTop();
                    right = cutout.getSafeInsetRight();
                    bottom = cutout.getSafeInsetBottom();
                }
            }
            view.setPadding(pad + left, pad + top, pad + right, pad + bottom);
            return insets;
        });

        // Header: icon, name, hint (and the actions, in landscape)
        LinearLayout header = new LinearLayout(this);
        header.setOrientation(LinearLayout.HORIZONTAL);
        header.setGravity(Gravity.CENTER_VERTICAL);
        ImageView logo = new ImageView(this);
        logo.setImageResource(BuildConfig.APP_ICON);
        // Rounded like the app icons (see Bundle::load_icon()).
        GradientDrawable logoShape = new GradientDrawable();
        logoShape.setCornerRadius(dp(48) * 10f / 57f);
        logo.setBackground(logoShape);
        logo.setClipToOutline(true);
        header.addView(logo, new LinearLayout.LayoutParams(dp(48), dp(48)));
        LinearLayout titles = new LinearLayout(this);
        titles.setOrientation(LinearLayout.VERTICAL);
        titles.setPadding(dp(12), 0, dp(12), 0);
        titles.addView(makeText(BuildConfig.APP_NAME, 22, TEXT, true));
        titles.addView(makeText("点击图标启动游戏 · 长按可管理", 13, TEXT_DIM, false));
        header.addView(titles, new LinearLayout.LayoutParams(0, -2, 1));
        content.addView(header, new LinearLayout.LayoutParams(-1, -2));

        LinearLayout actions = new LinearLayout(this);
        actions.setOrientation(LinearLayout.HORIZONTAL);
        actions.setGravity(Gravity.CENTER_VERTICAL);
        int buttonHeight = isLandscape() ? dp(44) : dp(46);
        actions.addView(makeButton("导入游戏", true, v -> startImport()), isLandscape()
            ? new LinearLayout.LayoutParams(-2, buttonHeight)
            : new LinearLayout.LayoutParams(0, buttonHeight, 1));
        ImageView settings = new ImageView(this);
        settings.setImageDrawable(new GearDrawable(TEXT));
        settings.setScaleType(ImageView.ScaleType.FIT_CENTER);
        int iconPadding = (buttonHeight - dp(24)) / 2;
        settings.setPadding(iconPadding, iconPadding, iconPadding, iconPadding);
        settings.setBackground(buttonBackground(0x2EFFFFFF, buttonHeight / 2f));
        settings.setContentDescription("设置");
        settings.setOnClickListener(v -> showQuickOptions());
        LinearLayout.LayoutParams settingsParams =
            new LinearLayout.LayoutParams(buttonHeight, buttonHeight);
        settingsParams.leftMargin = dp(10);
        actions.addView(settings, settingsParams);
        if (isLandscape()) {
            header.addView(actions);
        }

        if (lastRunCrashed()) {
            TextView banner = makeText(
                "上次游戏出错退出了，点这里查看日志", 14, 0xFF3A2A00, true);
            banner.setGravity(Gravity.CENTER);
            banner.setPadding(dp(14), dp(10), dp(14), dp(10));
            banner.setBackground(buttonBackground(WARNING, dp(12)));
            banner.setOnClickListener(v -> showLog());
            LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(-1, -2);
            params.topMargin = dp(14);
            content.addView(banner, params);
        }
        updateSlot = new FrameLayout(this);
        content.addView(updateSlot, new LinearLayout.LayoutParams(-1, -2));
        showUpdateBanner();

        View divider = new View(this);
        divider.setBackgroundColor(0x33FFFFFF);
        LinearLayout.LayoutParams dividerParams = new LinearLayout.LayoutParams(-1, dp(1));
        dividerParams.topMargin = dp(14);
        content.addView(divider, dividerParams);

        // The apps, or a message where there are none
        FrameLayout middle = new FrameLayout(this);
        content.addView(middle, new LinearLayout.LayoutParams(-1, 0, 1));

        GridView grid = new GridView(this);
        grid.setNumColumns(GridView.AUTO_FIT);
        grid.setColumnWidth(dp(96));
        grid.setStretchMode(GridView.STRETCH_COLUMN_WIDTH);
        grid.setVerticalSpacing(dp(12));
        grid.setPadding(0, dp(14), 0, dp(14));
        grid.setClipToPadding(false);
        grid.setSelector(new ColorDrawable(Color.TRANSPARENT));
        grid.setVerticalScrollBarEnabled(false);
        adapter = new AppAdapter();
        grid.setAdapter(adapter);
        grid.setOnItemClickListener((parent, view, position, id) -> launch(apps.get(position)));
        grid.setOnItemLongClickListener((parent, view, position, id) -> {
            showAppMenu(apps.get(position));
            return true;
        });
        middle.addView(grid, new FrameLayout.LayoutParams(-1, -1));

        LinearLayout empty = new LinearLayout(this);
        empty.setOrientation(LinearLayout.VERTICAL);
        empty.setGravity(Gravity.CENTER);
        statusText = makeText("", 16, TEXT_DIM, false);
        statusText.setGravity(Gravity.CENTER);
        statusText.setLineSpacing(0, 1.2f);
        empty.addView(statusText);
        importBigButton = makeButton("导入游戏", true, v -> startImport());
        LinearLayout.LayoutParams bigParams = new LinearLayout.LayoutParams(dp(200), dp(50));
        bigParams.topMargin = dp(20);
        empty.addView(importBigButton, bigParams);
        middle.addView(empty, new FrameLayout.LayoutParams(-1, -1));

        if (!isLandscape()) {
            content.addView(actions, new LinearLayout.LayoutParams(-1, -2));
        }

        TextView version = makeText(
            new String(nativeVersion(), StandardCharsets.UTF_8), 11, 0x80FFFFFF, false);
        version.setGravity(Gravity.CENTER);
        version.setPadding(0, dp(8), 0, 0);
        content.addView(version, new LinearLayout.LayoutParams(-1, -2));

        setContentView(root);
        updateStatus();
    }

    private void updateStatus() {
        if (statusText == null) {
            return;
        }
        boolean showMessage = loading || apps.isEmpty();
        ((View) statusText.getParent()).setVisibility(showMessage ? View.VISIBLE : View.GONE);
        if (loading) {
            statusText.setText("正在读取游戏列表…");
            importBigButton.setVisibility(View.GONE);
        } else {
            statusText.setText("还没有游戏\n\n点击「导入游戏」，选择 .ipa 文件即可。");
            importBigButton.setVisibility(View.VISIBLE);
        }
    }

    private class AppAdapter extends BaseAdapter {
        @Override
        public int getCount() {
            return apps.size();
        }

        @Override
        public Object getItem(int position) {
            return apps.get(position);
        }

        @Override
        public long getItemId(int position) {
            return position;
        }

        @Override
        public View getView(int position, View convertView, ViewGroup parent) {
            LinearLayout item;
            ImageView icon;
            TextView name;
            if (convertView instanceof LinearLayout) {
                item = (LinearLayout) convertView;
                icon = (ImageView) item.getChildAt(0);
                name = (TextView) item.getChildAt(1);
            } else {
                item = new LinearLayout(LauncherActivity.this);
                item.setOrientation(LinearLayout.VERTICAL);
                item.setGravity(Gravity.CENTER_HORIZONTAL);
                item.setPadding(dp(4), dp(8), dp(4), dp(8));
                item.setBackground(buttonBackground(Color.TRANSPARENT, dp(16)));
                icon = new ImageView(LauncherActivity.this);
                icon.setScaleType(ImageView.ScaleType.FIT_CENTER);
                item.addView(icon, new LinearLayout.LayoutParams(dp(68), dp(68)));
                name = makeText("", 13, TEXT, false);
                name.setGravity(Gravity.CENTER);
                name.setMaxLines(2);
                name.setEllipsize(TextUtils.TruncateAt.END);
                name.setShadowLayer(dp(2), 0, dp(1), 0x99000000);
                LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(-1, -2);
                params.topMargin = dp(6);
                item.addView(name, params);
            }
            App app = apps.get(position);
            if (app.icon != null) {
                icon.setImageBitmap(app.icon);
                icon.setBackground(null);
            } else {
                GradientDrawable placeholder = new GradientDrawable();
                placeholder.setColor(0x40FFFFFF);
                placeholder.setCornerRadius(dp(68) * 10f / 57f);
                icon.setImageDrawable(null);
                icon.setBackground(placeholder);
            }
            name.setText(app.name);
            return item;
        }
    }

    // ---- Actions ----

    private void launch(App app) {
        List<String> args = new ArrayList<>();
        args.add(app.path);
        args.addAll(quickOptions.toArgs());
        Intent intent = new Intent(this, MainActivity.class);
        intent.putExtra(MainActivity.EXTRA_ARGS, args.toArray(new String[0]));
        startActivity(intent);
    }

    private void showAppMenu(final App app) {
        String[] items = {"导入存档", "导出存档", "删除游戏"};
        new AlertDialog.Builder(this, android.R.style.Theme_Material_Dialog_Alert)
            .setTitle(app.name)
            .setItems(items, (dialog, which) -> {
                if (which == 2) {
                    confirmDelete(app);
                } else if (app.bundleId == null || app.bundleId.isEmpty()) {
                    Toast.makeText(this, "无法识别这个游戏，不能处理存档。", Toast.LENGTH_LONG).show();
                } else if (which == 0) {
                    confirmImportSave(app);
                } else {
                    startExportSave(app);
                }
            })
            .show();
    }

    private void confirmDelete(final App app) {
        new AlertDialog.Builder(this, android.R.style.Theme_Material_Dialog_Alert)
            .setTitle("删除「" + app.name + "」？")
            .setMessage("会删除游戏文件，存档会保留。")
            .setPositiveButton("删除", (dialog, which) -> {
                if (deleteRecursively(new File(app.path))) {
                    Toast.makeText(this, "已删除。", Toast.LENGTH_SHORT).show();
                } else {
                    Toast.makeText(this, "删除失败。", Toast.LENGTH_LONG).show();
                }
                rescan();
            })
            .setNegativeButton("取消", null)
            .show();
    }

    private static boolean deleteRecursively(File file) {
        File[] children = file.isDirectory() ? file.listFiles() : null;
        if (children != null) {
            for (File child : children) {
                deleteRecursively(child);
            }
        }
        return file.delete();
    }

    private void showLog() {
        byte[] tail = readFile(new File(getBaseDir(), LOG_FILE), 48 * 1024);
        final String log = tail != null ? new String(tail, StandardCharsets.UTF_8) : "";
        showTextDialog("运行日志（" + LOG_FILE + "）", log, "分享", () -> {
            Intent intent = new Intent(Intent.ACTION_SEND);
            intent.setType("text/plain");
            intent.putExtra(Intent.EXTRA_TEXT, log);
            try {
                startActivity(Intent.createChooser(intent, "分享日志"));
            } catch (ActivityNotFoundException e) {
                Toast.makeText(this, "没有可以分享的应用。", Toast.LENGTH_LONG).show();
            }
        });
    }

    private void showCopyright() {
        showTextDialog("版权信息",
            new String(nativeCopyrightText(getAssets()), StandardCharsets.UTF_8), null, null);
    }

    /** A full-screen dialog showing a long monospace text. */
    private void showTextDialog(String title, String text, String extraButton, Runnable extra) {
        final Dialog dialog = new Dialog(this, android.R.style.Theme_Material_NoActionBar_Fullscreen);
        LinearLayout layout = new LinearLayout(this);
        layout.setOrientation(LinearLayout.VERTICAL);
        layout.setBackgroundColor(0xFF15192E);
        int pad = dp(16);
        layout.setPadding(pad, pad, pad, pad);

        LinearLayout bar = new LinearLayout(this);
        bar.setGravity(Gravity.CENTER_VERTICAL);
        bar.addView(makeText(title, 18, TEXT, true), new LinearLayout.LayoutParams(0, -2, 1));
        if (extraButton != null) {
            LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(-2, dp(40));
            params.rightMargin = dp(10);
            bar.addView(makeButton(extraButton, false, v -> extra.run()), params);
        }
        bar.addView(makeButton("关闭", true, v -> dialog.dismiss()),
            new LinearLayout.LayoutParams(-2, dp(40)));
        layout.addView(bar);

        ScrollView scroll = new ScrollView(this);
        TextView body = makeText(text, 12, 0xE6FFFFFF, false);
        body.setTypeface(Typeface.MONOSPACE);
        body.setPadding(0, dp(12), 0, dp(12));
        scroll.addView(body);
        layout.addView(scroll, new LinearLayout.LayoutParams(-1, 0, 1));
        if (extra != null) {
            // Logs are most useful from the end.
            scroll.post(() -> scroll.fullScroll(View.FOCUS_DOWN));
        }

        dialog.setContentView(layout);
        dialog.show();
    }

    // ---- Quick options ----

    private void showQuickOptions() {
        LinearLayout layout = new LinearLayout(this);
        layout.setOrientation(LinearLayout.VERTICAL);
        int pad = dp(20);
        layout.setPadding(pad, dp(8), pad, dp(8));

        addSegments(layout, "画面滤镜", new String[]{"平滑", "锐利", "像素", "高清"},
            quickOptions.filter, i -> quickOptions.filter = i);
        addSegments(layout, "画面比例", new String[]{"原始", "拉伸", "填满", "整数倍"},
            quickOptions.aspect, i -> quickOptions.aspect = i);
        addSegments(layout, "屏幕方向", new String[]{"默认", "横向 ←", "横向 →", "倒置"},
            quickOptions.orientation, i -> quickOptions.orientation = i);
        addSegments(layout, "放大渲染（实验性）", new String[]{"默认", "关", "2×", "3×", "4×"},
            quickOptions.scaleHack, i -> quickOptions.scaleHack = i);
        addSwitch(layout, "游戏时保持亮屏", quickOptions.keepScreenOn,
            b -> quickOptions.keepScreenOn = b);
        addSwitch(layout, "用摇杆模拟重力感应", quickOptions.tiltControls,
            b -> quickOptions.tiltControls = b);
        addSwitch(layout, "允许游戏联网", quickOptions.network, b -> quickOptions.network = b);
        // A launcher setting, not a touchHLE option, so it's not in the file.
        final SharedPreferences prefs = getSharedPreferences(PREFS, MODE_PRIVATE);
        addSwitch(layout, "自动检查更新", prefs.getBoolean(PREF_CHECK_UPDATES, true),
            b -> prefs.edit().putBoolean(PREF_CHECK_UPDATES, b).apply());

        TextView copyright = makeButton("版权信息", false, null);
        LinearLayout.LayoutParams copyrightParams = new LinearLayout.LayoutParams(-1, dp(44));
        copyrightParams.topMargin = dp(16);
        layout.addView(copyright, copyrightParams);

        ScrollView scroll = new ScrollView(this);
        scroll.addView(layout);
        final AlertDialog dialog = new AlertDialog.Builder(
                this, android.R.style.Theme_Material_Dialog_Alert)
            .setTitle("设置")
            .setView(scroll)
            .setPositiveButton("完成", null)
            .setOnDismissListener(d -> quickOptions.save(getBaseDir()))
            .create();
        copyright.setOnClickListener(v -> showCopyright());
        dialog.show();
    }

    private interface IntSetter {
        void set(int value);
    }

    private interface BoolSetter {
        void set(boolean value);
    }

    private void addSegments(LinearLayout parent, String label, String[] titles, int selected,
                             final IntSetter setter) {
        TextView labelView = makeText(label, 14, TEXT_DIM, false);
        labelView.setPadding(0, dp(14), 0, dp(6));
        parent.addView(labelView);

        final LinearLayout row = new LinearLayout(this);
        row.setOrientation(LinearLayout.HORIZONTAL);
        for (int i = 0; i < titles.length; i++) {
            final int index = i;
            TextView segment = makeText(titles[i], 14, TEXT, false);
            segment.setGravity(Gravity.CENTER);
            segment.setSingleLine(true);
            segment.setOnClickListener(v -> {
                setter.set(index);
                styleSegments(row, index);
            });
            LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(0, dp(38), 1);
            if (i > 0) {
                params.leftMargin = dp(6);
            }
            row.addView(segment, params);
        }
        styleSegments(row, selected);
        parent.addView(row);
    }

    private void styleSegments(LinearLayout row, int selected) {
        for (int i = 0; i < row.getChildCount(); i++) {
            TextView segment = (TextView) row.getChildAt(i);
            boolean on = i == selected;
            segment.setBackground(buttonBackground(on ? ACCENT : 0x26FFFFFF, dp(10)));
            segment.setTypeface(on ? Typeface.DEFAULT_BOLD : Typeface.DEFAULT);
        }
    }

    private void addSwitch(LinearLayout parent, String label, boolean checked,
                           final BoolSetter setter) {
        Switch view = new Switch(this);
        view.setText(label);
        view.setTextSize(TypedValue.COMPLEX_UNIT_SP, 15);
        view.setChecked(checked);
        view.setMinHeight(dp(48));
        view.setOnCheckedChangeListener((button, isChecked) -> setter.set(isChecked));
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(-1, -2);
        params.topMargin = dp(8);
        parent.addView(view, params);
    }

    /**
     * The options from the "quick options" dialog. They're remembered in the
     * same file, and turned into the same command-line options, as in the app
     * picker (src/environment/app_picker.rs).
     */
    static class QuickOptions {
        static final String FILE = "touchHLE_quick_options.txt";
        static final String[] ASPECT = {null, "stretch", "fill", "integer"};
        static final String[] FILTER = {"smooth", "sharp", "pixel", "hq"};
        static final String[] ORIENTATION =
            {null, "--landscape-left", "--landscape-right", "--upside-down"};

        int scaleHack = 0; // 0: default, otherwise the --scale-hack= value
        int orientation = 0;
        int aspect = 0;
        int filter = 1; // sharp is the default on Android (see src/options.rs)
        boolean keepScreenOn = true;
        boolean tiltControls = true;
        boolean network = false;

        static int indexOf(String[] values, String value) {
            for (int i = 0; i < values.length; i++) {
                if (value.equals(values[i])) {
                    return i;
                }
            }
            return -1;
        }

        static QuickOptions load(File baseDir) {
            QuickOptions options = new QuickOptions();
            byte[] bytes = readFile(new File(baseDir, FILE), 64 * 1024);
            if (bytes == null) {
                return options;
            }
            for (String arg : new String(bytes, StandardCharsets.UTF_8).split("\n")) {
                arg = arg.trim();
                try {
                    if (arg.startsWith("--scale-hack=")) {
                        int value = Integer.parseInt(arg.substring("--scale-hack=".length()));
                        options.scaleHack = value >= 1 && value <= 4 ? value : 0;
                    } else if (arg.startsWith("--aspect=")) {
                        options.aspect = Math.max(0, indexOf(ASPECT, arg.substring(9)));
                    } else if (arg.startsWith("--filter=")) {
                        int i = indexOf(FILTER, arg.substring(9));
                        options.filter = i >= 0 ? i : options.filter;
                    } else if (indexOf(ORIENTATION, arg) >= 0) {
                        options.orientation = indexOf(ORIENTATION, arg);
                    } else if (arg.equals("--disable-analog-stick-tilt-controls")) {
                        options.tiltControls = false;
                    } else if (arg.equals("--allow-network-access")) {
                        options.network = true;
                    } else if (arg.equals("--keep-screen-on")) {
                        options.keepScreenOn = true;
                    } else if (arg.equals("--no-keep-screen-on")) {
                        options.keepScreenOn = false;
                    }
                } catch (NumberFormatException e) {
                    // Ignore it.
                }
            }
            return options;
        }

        List<String> toArgs() {
            List<String> args = new ArrayList<>();
            if (scaleHack != 0) {
                args.add("--scale-hack=" + scaleHack);
            }
            if (ORIENTATION[orientation] != null) {
                args.add(ORIENTATION[orientation]);
            }
            if (ASPECT[aspect] != null) {
                args.add("--aspect=" + ASPECT[aspect]);
            }
            args.add("--filter=" + FILTER[filter]);
            if (!tiltControls) {
                args.add("--disable-analog-stick-tilt-controls");
            }
            if (network) {
                args.add("--allow-network-access");
            }
            args.add(keepScreenOn ? "--keep-screen-on" : "--no-keep-screen-on");
            return args;
        }

        void save(File baseDir) {
            try (OutputStream out = new FileOutputStream(new File(baseDir, FILE))) {
                out.write(TextUtils.join("\n", toArgs()).getBytes(StandardCharsets.UTF_8));
            } catch (Exception e) {
                // Not essential.
            }
        }
    }

    // ---- Importing ----

    private void startImport() {
        Intent[] attempts = {pickIntent(Intent.ACTION_OPEN_DOCUMENT),
                             pickIntent(Intent.ACTION_GET_CONTENT)};
        for (Intent intent : attempts) {
            try {
                startActivityForResult(intent, REQUEST_IMPORT);
                return;
            } catch (ActivityNotFoundException | SecurityException e) {
                // Try the next one.
            }
        }
        Toast.makeText(this, "无法打开系统的文件选择器。", Toast.LENGTH_LONG).show();
    }

    private static Intent pickIntent(String action) {
        Intent intent = new Intent(action);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        // .ipa files have no well-known MIME type, so allow anything.
        intent.setType("*/*");
        intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true);
        return intent;
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode == REQUEST_IMPORT_SAVE || requestCode == REQUEST_EXPORT_SAVE) {
            if (resultCode == RESULT_OK && data != null && data.getData() != null
                && saveBundleId != null) {
                if (requestCode == REQUEST_IMPORT_SAVE) {
                    importSave(data.getData());
                } else {
                    exportSave(data.getData());
                }
            }
            return;
        }
        if (requestCode != REQUEST_IMPORT || resultCode != RESULT_OK || data == null) {
            return;
        }
        final List<Uri> uris = new ArrayList<>();
        ClipData clip = data.getClipData();
        if (clip != null) {
            for (int i = 0; i < clip.getItemCount(); i++) {
                uris.add(clip.getItemAt(i).getUri());
            }
        } else if (data.getData() != null) {
            uris.add(data.getData());
        }
        if (uris.isEmpty()) {
            return;
        }

        loading = true;
        updateStatus();
        statusText.setText("正在导入…");
        // Copying can take a while for big files, so don't block the UI thread.
        new Thread(() -> {
            final String message = importAll(uris);
            runOnUiThread(() -> {
                Toast.makeText(this, message, Toast.LENGTH_LONG).show();
                rescan();
            });
        }).start();
    }

    private String importAll(List<Uri> uris) {
        File appsDir = getAppsDir();
        appsDir.mkdirs();
        int imported = 0;
        List<String> skipped = new ArrayList<>();
        List<String> failed = new ArrayList<>();
        for (Uri uri : uris) {
            String name = displayName(uri);
            if (name == null || !name.toLowerCase(Locale.ROOT).endsWith(".ipa")) {
                skipped.add(name != null ? name : String.valueOf(uri.getLastPathSegment()));
                continue;
            }
            // Don't let a strange name escape the apps directory.
            name = name.replace('/', '_').replace('\\', '_');
            try {
                copy(uri, new File(appsDir, name));
                imported++;
            } catch (Exception e) {
                failed.add(name);
            }
        }

        StringBuilder message = new StringBuilder();
        message.append("已导入 ").append(imported).append(" 个游戏。");
        if (!skipped.isEmpty()) {
            message.append("\n不是 .ipa 文件，已跳过：").append(TextUtils.join("、", skipped));
        }
        if (!failed.isEmpty()) {
            message.append("\n导入失败：").append(TextUtils.join("、", failed));
        }
        return message.toString();
    }

    private String displayName(Uri uri) {
        try (Cursor cursor = getContentResolver().query(
                uri, new String[]{OpenableColumns.DISPLAY_NAME}, null, null, null)) {
            if (cursor != null && cursor.moveToFirst() && !cursor.isNull(0)) {
                return cursor.getString(0);
            }
        } catch (Exception e) {
            // Fall back to the URI below.
        }
        return uri.getLastPathSegment();
    }

    private void copy(Uri uri, File dest) throws Exception {
        // Copy to a temporary name first, so a partially copied file is never
        // listed (or run).
        File temp = new File(dest.getParentFile(), dest.getName() + ".importing");
        try (InputStream in = getContentResolver().openInputStream(uri);
             OutputStream out = new FileOutputStream(temp)) {
            if (in == null) {
                throw new Exception("Couldn't open " + uri);
            }
            byte[] buffer = new byte[1 << 16];
            int count;
            while ((count = in.read(buffer)) != -1) {
                out.write(buffer, 0, count);
            }
        } catch (Exception e) {
            temp.delete();
            throw e;
        }
        if (dest.exists() && !dest.delete()) {
            temp.delete();
            throw new Exception("Couldn't replace " + dest);
        }
        if (!temp.renameTo(dest)) {
            temp.delete();
            throw new Exception("Couldn't rename " + temp);
        }
    }

    // ---- Saves ----

    private File getSaveDir(String bundleId) {
        return new File(new File(getBaseDir(), SANDBOX_DIR), bundleId);
    }

    private static String timestamp() {
        return new SimpleDateFormat("yyyyMMdd_HHmmss", Locale.ROOT).format(new Date());
    }

    /// The parts of an app's data that make up its saves (not tmp or caches).
    private static boolean isSavePath(String relative) {
        return (relative.equals("Documents") || relative.startsWith("Documents/")
                || relative.equals("Library") || relative.startsWith("Library/"))
            && !relative.equals("Library/Caches") && !relative.startsWith("Library/Caches/");
    }

    /// Writes an app's saves to `out` as a zip. Returns the number of files
    /// written and (in `count[1]`) skipped because they couldn't be read.
    private int[] writeSaveZip(String bundleId, OutputStream out) throws Exception {
        File saveDir = getSaveDir(bundleId);
        int[] count = {0, 0};
        try (ZipOutputStream zip = new ZipOutputStream(out)) {
            zip.putNextEntry(new ZipEntry(SAVE_MARKER));
            zip.write(("touchHLE save\nbundle=" + bundleId + "\n").getBytes(StandardCharsets.UTF_8));
            zip.closeEntry();
            for (String top : new String[]{"Documents", "Library"}) {
                addToZip(zip, new File(saveDir, top), top, count);
            }
        }
        return count;
    }

    private static void addToZip(ZipOutputStream zip, File file, String relative, int[] count)
            throws Exception {
        if (!isSavePath(relative)) {
            return;
        }
        File[] children = file.listFiles();
        if (file.isDirectory()) {
            if (children != null) {
                for (File child : children) {
                    addToZip(zip, child, relative + "/" + child.getName(), count);
                }
            }
            return;
        }
        if (!file.isFile()) {
            return;
        }
        InputStream in;
        try {
            in = new FileInputStream(file);
        } catch (Exception e) {
            // E.g. a save copied in by hand that touchHLE isn't allowed to read.
            count[1]++;
            return;
        }
        try (InputStream input = in) {
            zip.putNextEntry(new ZipEntry(relative));
            byte[] buffer = new byte[1 << 16];
            int n;
            while ((n = input.read(buffer)) != -1) {
                zip.write(buffer, 0, n);
            }
            zip.closeEntry();
        }
        count[0]++;
    }

    private static boolean hasFiles(File dir) {
        File[] children = dir.listFiles();
        if (children == null) {
            return false;
        }
        for (File child : children) {
            if (child.isFile() || hasFiles(child)) {
                return true;
            }
        }
        return false;
    }

    private boolean hasSaves(String bundleId) {
        File saveDir = getSaveDir(bundleId);
        return hasFiles(new File(saveDir, "Documents")) || hasFiles(new File(saveDir, "Library"));
    }

    private void startExportSave(App app) {
        if (!hasSaves(app.bundleId)) {
            Toast.makeText(this, "「" + app.name + "」还没有存档。", Toast.LENGTH_LONG).show();
            return;
        }
        saveBundleId = app.bundleId;
        saveAppName = app.name;
        Intent intent = new Intent(Intent.ACTION_CREATE_DOCUMENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        intent.setType("application/zip");
        intent.putExtra(Intent.EXTRA_TITLE, app.bundleId + "_save_" + timestamp() + ".zip");
        try {
            startActivityForResult(intent, REQUEST_EXPORT_SAVE);
        } catch (ActivityNotFoundException | SecurityException e) {
            // No system "save as" screen: save it in touchHLE's own folder.
            exportSaveToFolder();
        }
    }

    private void exportSave(final Uri uri) {
        final String bundleId = saveBundleId;
        final String name = saveAppName;
        new Thread(() -> {
            String message;
            try (OutputStream out = getContentResolver().openOutputStream(uri)) {
                if (out == null) {
                    throw new Exception("Couldn't open " + uri);
                }
                message = exportMessage(name, writeSaveZip(bundleId, out), "");
            } catch (Exception e) {
                message = "导出存档失败：" + e.getMessage();
            }
            final String text = message;
            runOnUiThread(() -> Toast.makeText(this, text, Toast.LENGTH_LONG).show());
        }).start();
    }

    private static String exportMessage(String name, int[] count, String where) {
        String message = "已导出「" + name + "」的存档（" + count[0] + " 个文件）。" + where;
        if (count[1] > 0) {
            message += "\n有 " + count[1] + " 个文件没有读取权限，已跳过。";
        }
        return message;
    }

    private void exportSaveToFolder() {
        final String bundleId = saveBundleId;
        final String name = saveAppName;
        new Thread(() -> {
            String message;
            File dir = new File(getBaseDir(), SAVE_EXPORTS_DIR);
            File file = new File(dir, bundleId + "_save_" + timestamp() + ".zip");
            try {
                dir.mkdirs();
                int[] count;
                try (OutputStream out = new FileOutputStream(file)) {
                    count = writeSaveZip(bundleId, out);
                }
                message = exportMessage(name, count,
                    "\n保存在 touchHLE 文件夹：" + SAVE_EXPORTS_DIR + "/" + file.getName());
            } catch (Exception e) {
                file.delete();
                message = "导出存档失败：" + e.getMessage();
            }
            final String text = message;
            runOnUiThread(() -> Toast.makeText(this, text, Toast.LENGTH_LONG).show());
        }).start();
    }

    private void confirmImportSave(final App app) {
        new AlertDialog.Builder(this, android.R.style.Theme_Material_Dialog_Alert)
            .setTitle("导入「" + app.name + "」的存档？")
            .setMessage("请选择一个导出的存档 zip。现有存档会先备份到 touchHLE 文件夹的 "
                + SAVE_BACKUPS_DIR + "，再被替换。\n\n导入前请先退出这个游戏。")
            .setPositiveButton("选择文件", (dialog, which) -> {
                saveBundleId = app.bundleId;
                saveAppName = app.name;
                Intent[] attempts = {new Intent(Intent.ACTION_OPEN_DOCUMENT),
                                     new Intent(Intent.ACTION_GET_CONTENT)};
                for (Intent intent : attempts) {
                    intent.addCategory(Intent.CATEGORY_OPENABLE);
                    intent.setType("*/*");
                    try {
                        startActivityForResult(intent, REQUEST_IMPORT_SAVE);
                        return;
                    } catch (ActivityNotFoundException | SecurityException e) {
                        // Try the next one.
                    }
                }
                Toast.makeText(this, "无法打开系统的文件选择器。", Toast.LENGTH_LONG).show();
            })
            .setNegativeButton("取消", null)
            .show();
    }

    /// Unpacks the zip into a fresh folder in the cache, then checks it and
    /// asks before replacing the app's saves with it.
    private void importSave(final Uri uri) {
        final String bundleId = saveBundleId;
        final String name = saveAppName;
        new Thread(() -> {
            final File staging = new File(getCacheDir(), "save_import");
            deleteRecursively(staging);
            String error = null;
            String zipBundleId = null;
            try {
                unzip(uri, staging);
                File root = saveRoot(staging);
                zipBundleId = readMarker(new File(staging, SAVE_MARKER));
                if (zipBundleId == null) {
                    zipBundleId = readMarker(new File(root, SAVE_MARKER));
                }
                if (!hasFiles(root)) {
                    error = "这个 zip 里没有存档文件。";
                }
            } catch (Exception e) {
                error = "无法读取这个文件，它是存档 zip 吗？";
            }
            final String failure = error;
            final String fromId = zipBundleId;
            runOnUiThread(() -> {
                if (failure != null) {
                    deleteRecursively(staging);
                    Toast.makeText(this, failure, Toast.LENGTH_LONG).show();
                } else if (fromId != null && !fromId.equals(bundleId)) {
                    new AlertDialog.Builder(this, android.R.style.Theme_Material_Dialog_Alert)
                        .setTitle("存档和游戏不一致")
                        .setMessage("这个存档来自另一个游戏（" + fromId + "），不是「" + name
                            + "」（" + bundleId + "）。仍然导入吗？")
                        .setPositiveButton("仍然导入", (d, w) -> applySave(staging, bundleId, name))
                        .setNegativeButton("取消", (d, w) -> deleteRecursively(staging))
                        .show();
                } else {
                    applySave(staging, bundleId, name);
                }
            });
        }).start();
    }

    private void applySave(final File staging, final String bundleId, final String name) {
        new Thread(() -> {
            String message;
            File saveDir = getSaveDir(bundleId);
            String step = "移走旧存档";
            try {
                // The old saves are moved into the backup folder rather than
                // copied and deleted: saves copied in by hand (e.g. with a file
                // manager) may not be readable or deletable by touchHLE, but
                // its own folders can still be moved.
                File backup = new File(new File(getBaseDir(), SAVE_BACKUPS_DIR),
                    bundleId + "_" + timestamp());
                moveAside(new File(saveDir, "Documents"), new File(backup, "Documents"));
                File[] libraryChildren = new File(saveDir, "Library").listFiles();
                if (libraryChildren != null) {
                    for (File child : libraryChildren) {
                        if (!child.getName().equals("Caches")) {
                            moveAside(child, new File(new File(backup, "Library"), child.getName()));
                        }
                    }
                }
                step = "写入新存档";
                File root = saveRoot(staging);
                int count = installSave(root, saveDir);
                message = "已导入「" + name + "」的存档（" + count + " 个文件）。";
            } catch (Exception e) {
                message = "导入存档失败（" + step + "时）：" + e.getMessage()
                    + "\n原来的存档在 touchHLE 文件夹的 " + SAVE_BACKUPS_DIR + " 里。";
            }
            deleteRecursively(staging);
            final String text = message;
            runOnUiThread(() -> Toast.makeText(this, text, Toast.LENGTH_LONG).show());
        }).start();
    }

    /// Moves `file` (if it exists) to `dest`, which must not exist yet.
    private static void moveAside(File file, File dest) throws Exception {
        if (!file.exists()) {
            return;
        }
        dest.getParentFile().mkdirs();
        if (!file.renameTo(dest)) {
            throw new Exception("无法移走旧存档 " + file.getName());
        }
    }

    /// Where the saves are in an unpacked zip: possibly inside a single folder
    /// (e.g. a zipped `touchHLE_sandbox/<bundle id>` folder).
    private static File saveRoot(File staging) {
        File root = staging;
        while (true) {
            File[] children = root.listFiles();
            if (children == null) {
                return root;
            }
            File onlyDir = null;
            int others = 0;
            for (File child : children) {
                if (child.getName().equals(SAVE_MARKER)) {
                    continue;
                }
                if (child.isDirectory() && onlyDir == null) {
                    onlyDir = child;
                } else {
                    others++;
                }
            }
            if (onlyDir == null || others > 0 || onlyDir.getName().equals("Documents")
                || onlyDir.getName().equals("Library")) {
                return root;
            }
            root = onlyDir;
        }
    }

    /// Moves unpacked saves into the app's folder: `Documents` and `Library`
    /// keep their place, anything else goes into `Documents`.
    private static int installSave(File root, File saveDir) throws Exception {
        int count = 0;
        File[] children = root.listFiles();
        if (children == null) {
            return 0;
        }
        boolean structured = new File(root, "Documents").isDirectory()
            || new File(root, "Library").isDirectory();
        for (File child : children) {
            String childName = child.getName();
            if (childName.equals(SAVE_MARKER)) {
                continue;
            }
            File dest;
            if (structured && (childName.equals("Documents") || childName.equals("Library"))) {
                dest = new File(saveDir, childName);
            } else {
                dest = new File(new File(saveDir, "Documents"), childName);
            }
            count += moveTree(child, dest);
        }
        return count;
    }

    private static int moveTree(File source, File dest) throws Exception {
        if (source.isDirectory()) {
            dest.mkdirs();
            int count = 0;
            File[] children = source.listFiles();
            if (children != null) {
                for (File child : children) {
                    count += moveTree(child, new File(dest, child.getName()));
                }
            }
            return count;
        }
        dest.getParentFile().mkdirs();
        if (dest.exists()) {
            dest.delete();
        }
        if (!source.renameTo(dest)) {
            try (InputStream in = new FileInputStream(source);
                 OutputStream out = new FileOutputStream(dest)) {
                byte[] buffer = new byte[1 << 16];
                int n;
                while ((n = in.read(buffer)) != -1) {
                    out.write(buffer, 0, n);
                }
            }
        }
        return 1;
    }

    private void unzip(Uri uri, File dest) throws Exception {
        String destPath = dest.getCanonicalPath() + File.separator;
        int entries = 0;
        try (InputStream in = getContentResolver().openInputStream(uri);
             ZipInputStream zip = new ZipInputStream(in)) {
            ZipEntry entry;
            while ((entry = zip.getNextEntry()) != null) {
                entries++;
                String entryName = entry.getName().replace('\\', '/');
                // Skip files macOS adds to zips.
                if (entryName.startsWith("__MACOSX/") || entryName.endsWith("/.DS_Store")
                    || entryName.equals(".DS_Store")) {
                    continue;
                }
                File file = new File(dest, entryName);
                // Don't let a strange name escape the folder.
                if (!file.getCanonicalPath().startsWith(destPath)) {
                    continue;
                }
                if (entry.isDirectory()) {
                    file.mkdirs();
                    continue;
                }
                file.getParentFile().mkdirs();
                try (OutputStream out = new FileOutputStream(file)) {
                    byte[] buffer = new byte[1 << 16];
                    int n;
                    while ((n = zip.read(buffer)) != -1) {
                        out.write(buffer, 0, n);
                    }
                }
            }
        }
        if (entries == 0) {
            throw new Exception("Not a zip file");
        }
    }

    /// The bundle identifier in an exported save's marker file, if any.
    private static String readMarker(File marker) {
        byte[] data = readFile(marker, 4096);
        if (data == null) {
            return null;
        }
        for (String line : new String(data, StandardCharsets.UTF_8).split("\n")) {
            if (line.startsWith("bundle=")) {
                return line.substring("bundle=".length()).trim();
            }
        }
        return null;
    }

    // ---- Updates ----

    /// The version numbers in e.g. "v0.3.12" or "v0.3.12-2-gabcdef", or null.
    private static int[] parseVersion(String version) {
        Matcher m = Pattern.compile("v?(\\d+)\\.(\\d+)\\.(\\d+)").matcher(version);
        if (!m.find()) {
            return null;
        }
        return new int[]{Integer.parseInt(m.group(1)), Integer.parseInt(m.group(2)),
                         Integer.parseInt(m.group(3))};
    }

    private static boolean isNewer(int[] a, int[] b) {
        for (int i = 0; i < 3; i++) {
            if (a[i] != b[i]) {
                return a[i] > b[i];
            }
        }
        return false;
    }

    /// Asks GitHub for the latest release in the background, and shows a
    /// banner if it's newer than this version. Failures (e.g. no network) are
    /// ignored.
    private void checkForUpdate() {
        if (!getSharedPreferences(PREFS, MODE_PRIVATE).getBoolean(PREF_CHECK_UPDATES, true)) {
            return;
        }
        final int[] current = parseVersion(new String(nativeVersion(), StandardCharsets.UTF_8));
        if (current == null) {
            return;
        }
        new Thread(() -> {
            try {
                HttpURLConnection connection =
                    (HttpURLConnection) new URL(LATEST_RELEASE_URL).openConnection();
                connection.setConnectTimeout(8000);
                connection.setReadTimeout(8000);
                connection.setRequestProperty("Accept", "application/vnd.github+json");
                connection.setRequestProperty("User-Agent", "touchHLE-UF");
                byte[] data;
                try (InputStream in = connection.getInputStream()) {
                    java.io.ByteArrayOutputStream out = new java.io.ByteArrayOutputStream();
                    byte[] buffer = new byte[1 << 14];
                    int n;
                    while ((n = in.read(buffer)) != -1 && out.size() < (1 << 20)) {
                        out.write(buffer, 0, n);
                    }
                    data = out.toByteArray();
                } finally {
                    connection.disconnect();
                }
                JSONObject release = new JSONObject(new String(data, StandardCharsets.UTF_8));
                String tag = release.getString("tag_name");
                int[] latest = parseVersion(tag);
                if (latest == null || !isNewer(latest, current)) {
                    return;
                }
                final UpdateInfo info = new UpdateInfo();
                info.version = tag;
                info.notes = release.optString("body", "");
                info.downloadUrl = release.optString("html_url", "");
                JSONArray assets = release.optJSONArray("assets");
                for (int i = 0; assets != null && i < assets.length(); i++) {
                    JSONObject asset = assets.getJSONObject(i);
                    if (asset.optString("name").endsWith("_Android_AArch64.apk")) {
                        info.downloadUrl = asset.optString("browser_download_url", info.downloadUrl);
                    }
                }
                runOnUiThread(() -> {
                    update = info;
                    showUpdateBanner();
                });
            } catch (Exception e) {
                // No network, GitHub unreachable, etc.: just don't show anything.
            }
        }).start();
    }

    private void showUpdateBanner() {
        if (updateSlot == null) {
            return;
        }
        updateSlot.removeAllViews();
        String ignored = getSharedPreferences(PREFS, MODE_PRIVATE).getString(PREF_IGNORED_VERSION, "");
        if (update == null || update.version.equals(ignored)) {
            return;
        }
        TextView banner = makeText("发现新版本 " + update.version + "，点这里更新", 14, TEXT, true);
        banner.setGravity(Gravity.CENTER);
        banner.setPadding(dp(14), dp(10), dp(14), dp(10));
        banner.setBackground(buttonBackground(ACCENT, dp(12)));
        banner.setOnClickListener(v -> showUpdateDialog());
        FrameLayout.LayoutParams params = new FrameLayout.LayoutParams(-1, -2);
        params.topMargin = dp(14);
        updateSlot.addView(banner, params);
    }

    /// The release notes are Markdown: keep the "Changes in ..." part (the
    /// rest describes the project and downloads), and drop the markup.
    private static String plainNotes(String notes) {
        notes = notes.replace("\r", "");
        int changes = notes.indexOf("Changes in ");
        if (changes >= 0) {
            notes = notes.substring(notes.lastIndexOf('\n', changes) + 1);
        }
        int built = notes.indexOf("\nBuilt from commit");
        if (built >= 0) {
            notes = notes.substring(0, built);
        }
        return notes.replaceAll("(?m)^#+\\s*", "")
            .replace("**", "")
            .replace("`", "")
            .replaceAll("\\[([^\\]]*)\\]\\([^)]*\\)", "$1")
            .trim();
    }

    private void showUpdateDialog() {
        final UpdateInfo info = update;
        if (info == null) {
            return;
        }
        new AlertDialog.Builder(this, android.R.style.Theme_Material_Dialog_Alert)
            .setTitle("发现新版本 " + info.version)
            .setMessage(plainNotes(info.notes))
            .setPositiveButton("下载安装包", (dialog, which) -> {
                try {
                    startActivity(new Intent(Intent.ACTION_VIEW, Uri.parse(info.downloadUrl)));
                } catch (ActivityNotFoundException e) {
                    Toast.makeText(this, "没有可以打开下载链接的浏览器。", Toast.LENGTH_LONG).show();
                }
            })
            .setNegativeButton("稍后", null)
            .setNeutralButton("忽略这个版本", (dialog, which) -> {
                getSharedPreferences(PREFS, MODE_PRIVATE).edit()
                    .putString(PREF_IGNORED_VERSION, info.version).apply();
                showUpdateBanner();
            })
            .show();
    }

    /// A gear icon for the settings button, drawn here rather than taken from
    /// an icon set.
    private static class GearDrawable extends Drawable {
        private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);

        GearDrawable(int color) {
            paint.setColor(color);
            paint.setStyle(Paint.Style.FILL);
        }

        @Override
        public void draw(Canvas canvas) {
            Rect bounds = getBounds();
            float cx = bounds.exactCenterX();
            float cy = bounds.exactCenterY();
            float outer = Math.min(bounds.width(), bounds.height()) / 2f;
            float inner = outer * 0.74f;
            int teeth = 8;
            Path path = new Path();
            path.setFillType(Path.FillType.EVEN_ODD);
            for (int i = 0; i < teeth; i++) {
                double center = 2 * Math.PI * i / teeth;
                double[] angles = {center - 0.30, center - 0.18, center + 0.18, center + 0.30};
                float[] radii = {inner, outer, outer, inner};
                for (int j = 0; j < 4; j++) {
                    float x = cx + (float) Math.cos(angles[j]) * radii[j];
                    float y = cy + (float) Math.sin(angles[j]) * radii[j];
                    if (i == 0 && j == 0) {
                        path.moveTo(x, y);
                    } else {
                        path.lineTo(x, y);
                    }
                }
            }
            path.close();
            path.addCircle(cx, cy, outer * 0.32f, Path.Direction.CW);
            canvas.drawPath(path, paint);
        }

        @Override
        public void setAlpha(int alpha) {
            paint.setAlpha(alpha);
        }

        @Override
        public void setColorFilter(ColorFilter filter) {
            paint.setColorFilter(filter);
        }

        @Override
        public int getOpacity() {
            return PixelFormat.TRANSLUCENT;
        }

        @Override
        public int getIntrinsicWidth() {
            return 48;
        }

        @Override
        public int getIntrinsicHeight() {
            return 48;
        }
    }
}
