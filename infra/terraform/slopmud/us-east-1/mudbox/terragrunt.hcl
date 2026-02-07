include "root" {
  path = find_in_parent_folders("root.hcl")
}

terraform {
  source = "../../modules/mudbox"
}

inputs = {
  name_prefix    = "mudbox"
  enable_compute = true
  os             = "debian12"
  instance_type  = "t3a.small"
  spot_max_price = ""

  zone_name        = "slopmud.com"
  record_name      = "mud"
  create_hosted_zone = true

  # Set this when you know what you want mud.slopmud.com to point at.
  # cname_target = "example.com"
}
