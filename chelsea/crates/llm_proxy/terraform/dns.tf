# ── Route53 Internal DNS ──────────────────────────────────────────────────────
# Creates tokens.internal.vers.sh → internal ALB
#
# If no hosted zone ID is provided, the zone and record are skipped.
# You can create the private hosted zone here or use an existing one.

resource "aws_route53_zone" "internal" {
  count = var.internal_zone_id == "" ? 1 : 0

  name = "internal.vers.sh"

  vpc {
    vpc_id = var.vpc_id
  }

  comment = "Private zone for internal Vers services"
}

locals {
  zone_id = var.internal_zone_id != "" ? var.internal_zone_id : aws_route53_zone.internal[0].zone_id
}

resource "aws_route53_record" "tokens" {
  zone_id = local.zone_id
  name    = var.internal_domain
  type    = "A"

  alias {
    name                   = aws_lb.internal.dns_name
    zone_id                = aws_lb.internal.zone_id
    evaluate_target_health = true
  }
}
