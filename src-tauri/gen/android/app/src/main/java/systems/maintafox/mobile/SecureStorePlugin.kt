package systems.maintafox.mobile

import android.app.Activity
import android.app.KeyguardManager
import android.content.Context
import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.security.keystore.StrongBoxUnavailableException
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import java.security.KeyStore
import java.security.MessageDigest
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

@InvokeArg
class NameArgs {
    lateinit var name: String
}

@InvokeArg
class SetArgs {
    lateinit var name: String
    lateinit var value: String
}

/**
 * Secret store of the Maintafox engine on Android (Rust side: `src/platform/secret_store.rs`).
 *
 * Each secret (database key, device secret, activation secrets, installation id) is encrypted with
 * AES-256-GCM under one key that lives in the Android Keystore and never leaves it. The ciphertext
 * sits in the app's no-backup storage, one file per secret: `[12-byte IV][ciphertext + tag]`, the
 * secret's name bound as associated data. The key requires an unlocked device (Android 9+), and
 * the store refuses to work at all when the phone has no screen lock (BYOD rule, advisor
 * 2026-10-23): the app then cannot activate or open its database.
 *
 * Only Rust calls these commands (`run_mobile_plugin`); the webview is granted no permission.
 */
@TauriPlugin
class SecureStorePlugin(private val activity: Activity) : Plugin(activity) {
    private companion object {
        const val KEY_ALIAS = "maintafox-secret-wrap-v1"
        const val ANDROID_KEYSTORE = "AndroidKeyStore"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val IV_LEN = 12
        const val TAG_BITS = 128
        const val NO_SCREEN_LOCK = "NO_SCREEN_LOCK"
    }

    private fun isDeviceSecure(): Boolean =
        (activity.getSystemService(Context.KEYGUARD_SERVICE) as KeyguardManager).isDeviceSecure

    private fun storeDir(): File = File(activity.noBackupFilesDir, "secure-store").apply { mkdirs() }

    private fun fileFor(name: String): File {
        val digest = MessageDigest.getInstance("SHA-256").digest(name.toByteArray(Charsets.UTF_8))
        return File(storeDir(), digest.joinToString("") { "%02x".format(it) } + ".bin")
    }

    private fun wrapKey(): SecretKey {
        val ks = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }
        (ks.getKey(KEY_ALIAS, null) as? SecretKey)?.let { return it }
        fun spec(strongBox: Boolean) =
            KeyGenParameterSpec.Builder(KEY_ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .setRandomizedEncryptionRequired(true)
                .apply {
                    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                        setUnlockedDeviceRequired(true)
                        if (strongBox) setIsStrongBoxBacked(true)
                    }
                }
                .build()
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, ANDROID_KEYSTORE)
        return try {
            generator.init(spec(strongBox = true))
            generator.generateKey()
        } catch (e: StrongBoxUnavailableException) {
            // No secure element on this phone: the TEE-backed Keystore is used instead.
            generator.init(spec(strongBox = false))
            generator.generateKey()
        }
    }

    @Command
    fun deviceSecure(invoke: Invoke) {
        invoke.resolve(JSObject().put("secure", isDeviceSecure()))
    }

    @Command
    fun get(invoke: Invoke) {
        if (!isDeviceSecure()) return invoke.reject("the phone has no screen lock", NO_SCREEN_LOCK)
        val args = invoke.parseArgs(NameArgs::class.java)
        try {
            val file = fileFor(args.name)
            if (!file.exists()) {
                invoke.resolve(JSObject().put("value", null))
                return
            }
            val blob = file.readBytes()
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.DECRYPT_MODE, wrapKey(), GCMParameterSpec(TAG_BITS, blob, 0, IV_LEN))
            cipher.updateAAD(args.name.toByteArray(Charsets.UTF_8))
            val plain = cipher.doFinal(blob, IV_LEN, blob.size - IV_LEN)
            invoke.resolve(JSObject().put("value", String(plain, Charsets.UTF_8)))
        } catch (e: Exception) {
            invoke.reject("cannot read secret: ${e.javaClass.simpleName}: ${e.message}")
        }
    }

    @Command
    fun set(invoke: Invoke) {
        if (!isDeviceSecure()) return invoke.reject("the phone has no screen lock", NO_SCREEN_LOCK)
        val args = invoke.parseArgs(SetArgs::class.java)
        try {
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.ENCRYPT_MODE, wrapKey())
            cipher.updateAAD(args.name.toByteArray(Charsets.UTF_8))
            val sealed = cipher.doFinal(args.value.toByteArray(Charsets.UTF_8))
            val target = fileFor(args.name)
            val tmp = File(target.parentFile, target.name + ".tmp")
            tmp.writeBytes(cipher.iv + sealed)
            if (!tmp.renameTo(target)) {
                tmp.delete()
                return invoke.reject("cannot write secret file")
            }
            invoke.resolve()
        } catch (e: Exception) {
            invoke.reject("cannot store secret: ${e.javaClass.simpleName}: ${e.message}")
        }
    }
}
