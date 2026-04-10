// SPDX-License-Identifier: GPL-2.0
/*
 * user_entropy.c - allow user-mode to donate entropy to the kernel
 *
 * Safety notes
 * ------------
 *  • We **never** call credit_entropy_bits(), so users cannot
 *    artificially inflate the entropy estimate.
 *  • add_device_randomness() is non-blocking and IRQ-safe.
 *  • The device is world-writable by default (see module parameter
 *    'mode'), but your distro policy may want to tighten this.
 */

#include <linux/version.h>
#include <linux/module.h>
#include <linux/init.h>
#include <linux/fs.h>
#include <linux/cdev.h>
#include <linux/device.h>
#include <linux/uaccess.h>          /* copy_from_user()           */
#include <linux/random.h>           /* add_device_randomness()    */
#include <linux/slab.h>             /* kmalloc(), kfree()         */

#define DEV_CLASS        "user_entropy"
#define DRV_NAME         "user_entropy"
#define DEV_NAME         "user_entropy"
#define ENTROPY_MAX_SZ   4096       /* upper bound per write()    */

static dev_t          devno;
static struct cdev    cdev;
static struct class  *drv_class;

/* -------------------------------------------------------------------- */
/*               Module parameters - configurable at load time          */
/* -------------------------------------------------------------------- */

/* Major/minor auto-alloc: let users override major= if they insist.    */
static int major;
module_param(major, int, 0);
MODULE_PARM_DESC(major,
        "Major device number to allocate (0 = dynamic).");

/* File-mode for /dev/user_entropy.  Default 0200 for ease of use.      */
static umode_t mode = 0200;
module_param(mode, ushort, 0200);
MODULE_PARM_DESC(mode,
        "File permissions for /dev/user_entropy (octal, e.g. 0200).");

/* -------------------------------------------------------------------- */
/*                     File-operations implementation                   */
/* -------------------------------------------------------------------- */

static ssize_t ue_write(struct file *file, const char __user *ubuf,
                        size_t len, loff_t *ppos)
{
        u8   *kbuf;
        ssize_t ret = 0;

        if (!len)
                return 0;

        /* Hard upper bound to cap CPU time and memory. */
        if (len > ENTROPY_MAX_SZ)
                return -EMSGSIZE;

        kbuf = kmalloc(len, GFP_KERNEL);
        if (!kbuf)
                return -ENOMEM;

        if (copy_from_user(kbuf, ubuf, len)) {
                ret = -EFAULT;
                goto out;
        }

        /*
         * The helper mixes the supplied bytes into the input pool but DOES
         * NOT increase the entropy count.  That is intentional: only kernel
         * subsystems with *intrinsic* unpredictability may credit bits.
         */
        add_device_randomness(kbuf, len);
        ret = len;                  /* report full consumption    */
out:
        kfree(kbuf);
        return ret;
}

/* Nothing special for open/close; still nice to record ownership.      */
static int ue_open(struct inode *inode, struct file *file)
{
        try_module_get(THIS_MODULE);
        return 0;
}

static int ue_release(struct inode *inode, struct file *file)
{
        module_put(THIS_MODULE);
        return 0;
}

/* Supported operations table.                                          */
static const struct file_operations ue_fops = {
        .owner   = THIS_MODULE,
        .write   = ue_write,
        .open    = ue_open,
        .release = ue_release,
#if LINUX_VERSION_CODE >= KERNEL_VERSION(6,12,0)
        .llseek  = noop_llseek,     /* disallow lseek              */
#else
        .llseek  = no_llseek,       /* disallow lseek              */
#endif
};

/* -------------------------------------------------------------------- */
/*                    Module init / exit scaffolding                    */
/* -------------------------------------------------------------------- */

static int __init ue_init(void)
{
        int err;

        /* 1. Allocate <major,minor>.  Allow manual major override.      */
        err = alloc_chrdev_region(&devno, 0, 1,
                                  DRV_NAME);
        if (err) {
                pr_err("%s: alloc_chrdev_region failed (%d)\n",
                       DRV_NAME, err);
                return err;
        }
        major = MAJOR(devno);       /* record actual major          */

        /* 2. Init + add cdev.                                          */
        cdev_init(&cdev, &ue_fops);
        cdev.owner = THIS_MODULE;

        err = cdev_add(&cdev, devno, 1);
        if (err) {
                pr_err("%s: cdev_add failed (%d)\n", DRV_NAME, err);
                goto err_chrdev;
        }

        /* 3. Create /sys/class/user_entropy and the /dev node.         */
#if LINUX_VERSION_CODE < KERNEL_VERSION(6,4,0)
        drv_class = class_create(THIS_MODULE, DEV_CLASS);
#else
        drv_class = class_create(THIS_MODULE->name);
#endif

        if (IS_ERR(drv_class)) {
                err = PTR_ERR(drv_class);
                pr_err("%s: class_create failed (%d)\n", DRV_NAME, err);
                goto err_cdev;
        }

        if (!device_create(drv_class, NULL, devno, NULL,
                           DEV_NAME)) {
                pr_err("%s: device_create failed\n", DRV_NAME);
                err = -EINVAL;
                goto err_class;
        }

        pr_info("%s: registered /dev/%s (major=%d, mode=%#o, "
                "write cap %u bytes)\n",
                DRV_NAME, DEV_NAME, major, mode, ENTROPY_MAX_SZ);
        return 0;

err_class:
        class_destroy(drv_class);
err_cdev:
        cdev_del(&cdev);
err_chrdev:
        unregister_chrdev_region(devno, 1);
        return err;
}

static void __exit ue_exit(void)
{
        device_destroy(drv_class, devno);
        class_destroy(drv_class);
        cdev_del(&cdev);
        unregister_chrdev_region(devno, 1);
        pr_info("%s: unloaded\n", DRV_NAME);
}

module_init(ue_init);
module_exit(ue_exit);

MODULE_DESCRIPTION("Feeds user-supplied bytes to add_device_randomness()");
MODULE_AUTHOR("Joe Mistachkin <joe@mistachkin.com>");
MODULE_LICENSE("GPL v2");
MODULE_VERSION("1.0");
